/* Copyright 2022-present University of Tuebingen, Chair of Communication Networks
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

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use log::{info, warn};
use rbfrt::util::{Loopback, Port, Speed};
use rbfrt::util::{PortManager, PrettyPrinter};
use rbfrt::{table, SwitchConnection};

mod mna;
use mna::MNAController;

use crate::mna::PacketGenerator;

/// Configure all testbed ports when the controller starts.
///
/// Keep this disabled when the ports have already been configured externally.
/// In that mode, the controller neither clears `$PORT` nor writes port settings,
/// avoiding link flaps during startup.
const CONFIGURE_PORTS: bool = false;

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    info!("Start controller...");

    let switch = SwitchConnection::builder("localhost", 50052)
        .device_id(0)
        .client_id(1)
        .p4_name("mna_smep")
        .connect()
        .await?;

    if CONFIGURE_PORTS {
        switch.clear_table("$PORT").await?;
    }

    let pm = PortManager::new(&switch).await;

    // ------------------------------------------------------------------
    // Testbed topology (single-switch loopback, see paper Section 6.3).
    //
    //   P4TG             SMEP
    //    1 <---100G---> 13 --.
    //    2 <---100G---> 14   |
    //    3 <---100G---> 15   +--> [PLR] --200--> [egress]
    //    4 <---100G---> 16 --'    (p.4)   cable    (p.12)
    //                                      11 <-> 12 (400G)
    //                              |
    //                              | on egress failure: 201, 202
    //                              '--> [bypass 1] --> [bypass 2]
    //                                    (p.5)            (p.6)
    //                                                        |
    //                                                        '--> delivery
    //                                                             label 300--303
    //                                                                  |
    //                                                                  '--> p.13--16 --> P4TG
    //
    // The egress link (label 200) is a physical 400G cable between front-panel
    // ports 11 and 12. Port 11 is on a different pipe from the PLR on port 4,
    // exercising the cross-pipe port-status synchronisation during repair-time
    // measurements. Only a real link raises a PHY port-down event that the pktgen
    // trigger detects, so the shutdownable egress link must be cabled, not an
    // internal loopback. The PLR forwards label 200 out port 11; the packet
    // arrives on port 12, the egress node. Pulling the cable, or disabling port
    // 11, is the failure shared by all four traffic streams.
    //
    // The PLR and two bypass hops are independent internal MAC-near loopback
    // ports: a packet sent to such a port re-enters the pipeline on the same
    // port. Labels 201 and 202 form the two-label bypass tunnel. Delivery labels
    // 300--303 select the P4TG return port corresponding to the stream on which
    // the packet arrived.
    // ------------------------------------------------------------------

    const PORTS_P4TG: [u32; 4] = [13, 14, 15, 16];
    const PORT_EGRESS_LINK: u32 = 11; // different-pipe cable out; shut off to fail
    const PORT_EGRESS_NODE: u32 = 12; // cable in: egress node
    const PORT_PLR: u32 = 4; // internal loopback: PLR
    const PORT_BYPASS_1: u32 = 5; // internal loopback: first bypass hop
    const PORT_BYPASS_2: u32 = 6; // internal loopback: second bypass hop
    const DELIVERY_LABELS: [u32; 4] = [300, 301, 302, 303];

    let egress_port = PORT_EGRESS_LINK;

    if CONFIGURE_PORTS {
        // Four front-panel ports to P4TG. They stay up during the measurement.
        let mut port_requests: Vec<Port> = PORTS_P4TG
            .iter()
            .copied()
            .map(|fp_port| Port::new(fp_port, 0).speed(Speed::BF_SPEED_100G))
            .collect();

        // A physical 400G cable between ports 11 and 12 forms the protected
        // egress link. These are real ports, not loopbacks, so pulling the cable
        // is a detectable PHY port-down.
        for fp_port in [PORT_EGRESS_LINK, PORT_EGRESS_NODE] {
            port_requests.push(
                Port::new(fp_port, 0)
                    .speed(Speed::BF_SPEED_400G)
                    .fec(rbfrt::util::FEC::BF_FEC_TYP_REED_SOLOMON),
            );
        }

        // Independent internal MAC-near loopback ports for the PLR and bypass.
        for fp_port in [PORT_PLR, PORT_BYPASS_1, PORT_BYPASS_2] {
            port_requests.push(
                Port::new(fp_port, 0)
                    .speed(Speed::BF_SPEED_400G)
                    .loopback(Loopback::BF_LPBK_MAC_NEAR)
                    .fec(rbfrt::util::FEC::BF_FEC_TYP_REED_SOLOMON),
            );
        }

        pm.add_ports(&switch, &port_requests).await?;
        info!("Ports of device configured.");
    } else {
        info!("Port configuration skipped; using existing device configuration.");
    }

    // Mapping of MPLS label to the egress front-panel port of the hop. Label 200
    // egresses onto the protected 400G cable. The two bypass labels use separate
    // 400G loopback ports. The four delivery labels return each stream on its
    // corresponding P4TG link.
    let mpls_label_to_egress_port_mapping = HashMap::from([
        (100, PORT_PLR),
        (200, egress_port),
        (201, PORT_BYPASS_1),
        (202, PORT_BYPASS_2),
        (DELIVERY_LABELS[0], PORTS_P4TG[0]),
        (DELIVERY_LABELS[1], PORTS_P4TG[1]),
        (DELIVERY_LABELS[2], PORTS_P4TG[2]),
        (DELIVERY_LABELS[3], PORTS_P4TG[3]),
    ]);
    let mut mna_controller =
        MNAController::new(mpls_label_to_egress_port_mapping, DELIVERY_LABELS, &pm);

    let tables: Vec<&str> = vec![
        mna::mna_controller::MPLS_LOOKUP_TABLE,
        //"ingress.mpls_c.verify_ttl",
        "ingress.mpls_c.mpls_lookup_table",
        "ingress.mpls_c.mpls_smep_lookup_table",
        "ingress.mpls_c.mna_c.mna_first_nas_c.mna_subsequent_opcode_0",
        "ingress.mpls_c.mna_c.mna_first_nas_c.mna_subsequent_opcode_1",
        "ingress.mpls_c.mna_c.mna_first_nas_c.mna_subsequent_opcode_2",
        "ingress.mpls_c.mna_c.mna_first_nas_c.mna_subsequent_opcode_3",
        "ingress.mpls_c.mna_c.mna_first_nas_c.mna_subsequent_opcode_4",
        "ingress.mpls_c.mna_c.mna_first_nas_c.mna_subsequent_opcode_5",
        "ingress.mpls_c.mna_c.mna_first_nas_c.mna_subsequent_opcode_6",
        "ingress.mpls_c.mna_c.mna_first_nas_c.mna_subsequent_opcode_7",
        "ingress.mpls_c.mna_c.mna_first_nas_c.mna_subsequent_opcode_8",
        "ingress.mpls_c.mna_c.mna_first_nas_c.mna_subsequent_opcode_9",
        "ingress.mpls_c.mna_c.mna_first_nas_c.mna_subsequent_opcode_10",
        "ingress.mpls_c.mna_c.mna_first_nas_c.mna_subsequent_opcode_11",
        "ingress.mpls_c.mna_c.mna_first_nas_c.mna_subsequent_opcode_12",
        "ingress.mpls_c.mna_c.mna_first_nas_c.mna_subsequent_opcode_13",
        "ingress.mpls_c.mna_c.mna_first_nas_c.mna_subsequent_opcode_14",
        "ingress.mpls_c.mna_c.mna_second_nas_c.mna_subsequent_opcode_0",
        "ingress.mpls_c.mna_c.mna_second_nas_c.mna_subsequent_opcode_1",
        "ingress.mpls_c.mna_c.mna_second_nas_c.mna_subsequent_opcode_2",
        "ingress.mpls_c.mna_c.mna_second_nas_c.mna_subsequent_opcode_3",
        "ingress.mpls_c.mna_c.mna_second_nas_c.mna_subsequent_opcode_4",
        "ingress.mpls_c.mna_c.mna_second_nas_c.mna_subsequent_opcode_5",
        "ingress.mpls_c.mna_c.mna_second_nas_c.mna_subsequent_opcode_6",
        "ingress.mpls_c.mna_c.mna_second_nas_c.mna_subsequent_opcode_7",
        "ingress.mpls_c.mna_c.mna_second_nas_c.mna_subsequent_opcode_8",
        "ingress.mpls_c.mna_c.mna_second_nas_c.mna_subsequent_opcode_9",
        "ingress.mpls_c.mna_c.mna_second_nas_c.mna_subsequent_opcode_10",
        "ingress.mpls_c.mna_c.mna_second_nas_c.mna_subsequent_opcode_11",
        "ingress.mpls_c.mna_c.mna_second_nas_c.mna_subsequent_opcode_12",
        "ingress.mpls_c.mna_c.mna_second_nas_c.mna_subsequent_opcode_13",
        "ingress.mpls_c.mna_c.mna_first_nas_c.mna_initial_opcode",
        "ingress.mpls_c.mna_c.mna_second_nas_c.mna_initial_opcode",
        "ingress.mpls_c.port_status",
        "ingress.mpls_c.pop_smep_labels",
    ];

    switch.clear_tables(tables).await?;
    PacketGenerator::delete_simple_multicast_group(&switch).await?;
    PacketGenerator::deactivate_traffic_gen_application(&switch).await?;

    let mut table_entries = mna_controller.init_mpls_lookup();
    //table_entries.extend(mna_controller.init_constant_entries());
    table_entries.extend(mna_controller.init_opcode_entries());
    table_entries.extend(mna_controller.init_pop_entries());

    switch.write_table_entries(table_entries).await?;

    PacketGenerator::fill_packet_buffer(&switch).await?;
    PacketGenerator::create_multicast_port_down_replication(&switch).await?;
    PacketGenerator::enable_pkt_gen(&switch).await?;

    let switch: Arc<SwitchConnection> = Arc::new(switch);
    let switch2 = Arc::clone(&switch);

    if CONFIGURE_PORTS {
        // Wait for the ports to come up before arming the port-down trigger. The
        // trigger is one-shot per port, so a link flap during bring-up would
        // otherwise consume it and a later manual shutdown would not fire.
        tokio::time::sleep(Duration::from_secs(20)).await;
    }

    tokio::spawn(async move {
        mna_controller.digest_monitor(switch2).await;
    });

    PacketGenerator::init_port_down_trigger(&switch, &pm).await?;
    PacketGenerator::activate_traffic_gen_applications(&switch).await?;

    info!("SMEP Detection initialized");

    let pp = PrettyPrinter::new();

    loop {
        // Read tables for debugging
        let table_to_check = "ingress.mpls_c.pop_smep_labels";
        let sync =
            table::Request::new(table_to_check).operation(table::TableOperation::SyncCounters);

        if switch.execute_operation(sync).await.is_err() {
            warn! {"Encountered error while synchronizing {table_to_check}."};
        }

        let req: table::Request = table::Request::new(table_to_check);

        let res = switch.get_table_entries(req).await?;

        pp.print_table(res)?;

        tokio::time::sleep(Duration::from_secs(3)).await;
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    env_logger::init();

    match run().await {
        Ok(_) => {}
        Err(e) => {
            warn!("Error: {e}");
        }
    }
}
