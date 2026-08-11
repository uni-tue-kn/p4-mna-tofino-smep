/* Copyright 2026-present University of Tuebingen, Chair of Communication Networks
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *   http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

/*
 * Fabian Ihle (fabian.ihle@uni-tuebingen.de)
*/

use log::info;
use rbfrt::util::PortManager;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use rbfrt::{
    table::MatchValue,
    table::{self, ToBytes},
    SwitchConnection,
};

use crate::mna::PacketGenerator;

pub const MPLS_LOOKUP_TABLE: &str = "ingress.mpls_c.mpls_lookup_table";
pub const SMEP_LOOKUP_TABLE: &str = "ingress.mpls_c.mpls_smep_lookup_table";
pub const SMEP_POP_TABLE: &str = "ingress.mpls_c.pop_smep_labels";
const PORT_DOWN_DIGEST_NAME: &str = "pipe.SwitchIngressDeparser.digest_port_down";

#[derive(Clone, Debug)]
pub struct MNAController {
    mpls_label_to_egress_port_mapping: HashMap<u32, u32>,
    /// Labels at the bottom of the stack that are popped as the last label,
    /// including an ether-type rewrite, before a packet is returned to P4TG.
    delivery_labels: HashSet<u32>,
}

impl MNAController {
    pub fn new<const N: usize>(
        mpls_label_to_egress_port_mapping: HashMap<u32, u32>,
        delivery_labels: [u32; N],
        pm: &PortManager,
    ) -> MNAController {
        let mut label_dev_port_mapping = HashMap::new();
        for (label, front_panel_port) in mpls_label_to_egress_port_mapping.iter() {
            let dev_port = pm.dev_port(*front_panel_port, 0).unwrap();
            label_dev_port_mapping.insert(*label, dev_port);
        }

        MNAController {
            mpls_label_to_egress_port_mapping: label_dev_port_mapping,
            delivery_labels: HashSet::from(delivery_labels),
        }
    }

    pub fn init_mpls_lookup(&self) -> Vec<table::Request> {
        let mut table_entries = vec![];

        for (mpls_label, egress_port) in &self.mpls_label_to_egress_port_mapping {
            // The delivery label is the last label above the payload. Popping it
            // must restore the IPv4 ether type, so a dedicated action is used.
            let mpls_action = if self.delivery_labels.contains(mpls_label) {
                "ingress.mpls_c.forward_and_pop_last_label"
            } else {
                "ingress.mpls_c.forward_and_pop"
            };

            let tbl_request = table::Request::new(MPLS_LOOKUP_TABLE)
                .match_key("ig_md.resubmit_needed", MatchValue::exact(0))
                .match_key("hdr.mpls.label", MatchValue::exact(*mpls_label))
                .action(mpls_action)
                .action_data("port", *egress_port);

            table_entries.push(tbl_request);

            let tbl_request = table::Request::new(SMEP_LOOKUP_TABLE)
                .match_key("ig_md.resubmit_needed", MatchValue::exact(0))
                .match_key("hdr.mpls_inbetween_0.label", MatchValue::exact(*mpls_label))
                .action("ingress.mpls_c.smep_forward_and_pop")
                .action_data("port", *egress_port);

            table_entries.push(tbl_request);
        }

        table_entries
    }

    pub fn _init_constant_entries(&self) -> Vec<table::Request> {
        let mut table_entries = vec![];

        // Drop on 0 TTL
        let tbl_request = table::Request::new("ingress.mpls_c.verify_ttl")
            .match_key("hdr.mpls.ttl", MatchValue::exact(0))
            .action("ingress.mpls_c.drop");
        table_entries.push(tbl_request);

        table_entries
    }

    pub fn init_opcode_entries(&self) -> Vec<table::Request> {
        let mut table_entries = vec![];

        // POP-N action for SMEP
        for i in 1..=4 {
            let action = "ingress.mpls_c.mna_c.mna_first_nas_c.pop_n_label".to_string();

            // Construct the ternary value dynamically based on i
            let value = i << 4; // place i in bits [7:4] --> POP-N parameter
            let mask = 0b1111111110000; // first 9 bits significant (RESERVED + POP_N), last 4 bits don't care (MOVE_N)

            let tbl_request =
                table::Request::new("ingress.mpls_c.mna_c.mna_first_nas_c.mna_initial_opcode")
                    .match_key("hdr.mna_initial_opcode.opcode", MatchValue::exact(96))
                    .match_key(
                        "hdr.mna_initial_opcode.data",
                        MatchValue::ternary(value, mask),
                    )
                    .match_key("hdr.mna_initial_opcode.nal", MatchValue::exact(0))
                    .action(&action);

            table_entries.push(tbl_request);
        }

        table_entries
    }

    pub fn init_pop_entries(&self) -> Vec<table::Request> {
        let mut table_entries = vec![];

        for i in 1..=4 {
            let action = format!("ingress.mpls_c.pop_{i}_label");
            let tbl_request = table::Request::new(SMEP_POP_TABLE)
                .match_key("ig_md.smep.pop_labels", MatchValue::exact(i))
                .action(&action);
            table_entries.push(tbl_request);
        }

        table_entries
    }

    pub async fn digest_monitor(&mut self, switch: Arc<SwitchConnection>) {
        info!("Starting listening for digests");
        while let Ok(digest) = &mut switch.digest_queue.recv() {
            if digest.name == PORT_DOWN_DIGEST_NAME {
                let data = &digest.data;
                let port_num = data.get("port").unwrap().to_u32();
                let pipe = data.get("pipe").unwrap().to_u32();

                info!("Port down event detected on port {port_num} pipe {pipe}!");
                match PacketGenerator::reenable_port_down_trigger(&switch, port_num).await {
                    Ok(_) => info!("Reenabled port down trigger on port {port_num}"),
                    Err(e) => eprintln!("Error re-enabling port down trigger: {e}"),
                }
            }
        }
    }
}
