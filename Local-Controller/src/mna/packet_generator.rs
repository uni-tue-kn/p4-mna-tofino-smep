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

use std::{sync::Arc, vec};

use log::info;
use rbfrt::{
    error::RBFRTError,
    table::{MatchValue, Request},
    util::PortManager,
    SwitchConnection,
};

pub const TG_PIPE_PORTS_TF2: [u16; 4] = [6, 134, 262, 390];
pub const PORT_CFG_TF2: &str = "tf2.pktgen.port_cfg";
pub const APP_CFG_TF2: &str = "tf2.pktgen.app_cfg";
pub const APP_BUFFER_CFG_TF2: &str = "tf2.pktgen.pkt_buffer";

pub struct PacketGenerator;

impl PacketGenerator {
    /// ! 1. Configure app_cfg table with desired parameters
    pub async fn activate_traffic_gen_applications(
        switch: &Arc<SwitchConnection>,
    ) -> Result<(), RBFRTError> {
        // Write MAT entries for APP configuration
        let mut update_requests: Vec<Request> = vec![];

        // Table writes default to all pipes (pipe_id 0xffff), so this single
        // port-down application is instantiated on every pipe with the pipe-local
        // pktgen source port 6. The pktgen port-down trigger is a per-pipe
        // feature, and this way it is armed on every pipe, so a port-down on any
        // pipe is detected and then replicated to all pipes for the register
        // synchronisation. Use .pipe(p) on the request to target a single pipe.
        let req = Request::new(APP_CFG_TF2)
            .match_key("app_id", MatchValue::exact(0)) // value from 0 to 7 / 15 which identifies the stream
            .action("trigger_port_down")
            .action_data("app_enable", true)
            .action_data("pkt_len", 64) // Packet size
            .action_data("batch_count_cfg", 0) // Batches - 1
            .action_data("packets_per_batch_cfg", 0) // Packets per batch - 1
            .action_data("pipe_local_source_port", TG_PIPE_PORTS_TF2[0]) // pipe-local pktgen port (6)
            .action_data("pkt_counter", 0)
            .action_data("batch_counter", 0)
            .action_data("trigger_counter", 0)
            .action_data("assigned_chnl_id", TG_PIPE_PORTS_TF2[0]);
        update_requests.push(req);

        switch.update_table_entries(update_requests).await?;

        Ok(())
    }

    pub async fn deactivate_traffic_gen_application(
        switch: &SwitchConnection,
    ) -> Result<(), RBFRTError> {
        // Write MAT entries for APP configuration
        let mut update_requests: Vec<Request> = vec![];

        // Traffic Gen for Port Down Event
        let req = Request::new(APP_CFG_TF2)
            .match_key("app_id", MatchValue::exact(0)) // value from 0 to 7 / 15 which identifies the stream
            .action("trigger_port_down")
            .action_data("app_enable", false);
        update_requests.push(req);

        switch.update_table_entries(update_requests).await?;

        Ok(())
    }

    pub async fn init_port_down_trigger(
        switch: &SwitchConnection,
        pm: &PortManager,
    ) -> Result<(), RBFRTError> {
        // Retrieve all configured ports
        let ports = pm.get_ports(switch).await?;

        let mut requests = vec![];

        // Enable packet down trigger for them
        for port in ports {
            let dev_port = port.get_dev_port().unwrap();
            let req = Request::new(PORT_CFG_TF2)
                .match_key("dev_port", MatchValue::exact(dev_port))
                .action_data("clear_port_down_enable", true);
            requests.push(req);
        }
        switch.update_table_entries(requests).await?;

        Ok(())
    }

    pub async fn reenable_port_down_trigger(
        switch: &Arc<SwitchConnection>,
        dev_port: u32,
    ) -> Result<(), RBFRTError> {
        /*
            The Packet Generator keeps state on whether it has already seen a port
            go down.  It will only trigger for a port the first time that port
            goes down; so if a port starts down, comes up, goes down, comes back
            up, and finally goes down a second time only a single event is
            triggered.  An API must be called to reset this state for a given port
            in the Packet Generator.
        */

        // The trigger is one-shot per port, and re-arming needs the pktgen
        // application to be cycled. The SDE rejects reprogramming an application
        // while it is enabled ("cannot program to a pipe while an application
        // has already been enabled"), so the sequence is: disable the
        // application, clear the port-down state of the port, enable it again.
        // The enable and disable writes only touch app_enable, which is allowed
        // on a running application, unlike a full reprogram.
        let disable_app = Request::new(APP_CFG_TF2)
            .match_key("app_id", MatchValue::exact(0))
            .action("trigger_port_down")
            .action_data("app_enable", false);
        switch.update_table_entry(disable_app).await?;

        let rearm_port = Request::new(PORT_CFG_TF2)
            .match_key("dev_port", MatchValue::exact(dev_port))
            .action_data("clear_port_down_enable", true);
        switch.update_table_entry(rearm_port).await?;

        let enable_app = Request::new(APP_CFG_TF2)
            .match_key("app_id", MatchValue::exact(0))
            .action("trigger_port_down")
            .action_data("app_enable", true);
        switch.update_table_entry(enable_app).await?;

        Ok(())
    }

    pub async fn create_multicast_port_down_replication(
        switch: &SwitchConnection,
    ) -> Result<(), RBFRTError> {
        let req = Request::new("$pre.node")
            .match_key("$MULTICAST_NODE_ID", MatchValue::exact(1))
            .action_data("$MULTICAST_RID", 1)
            .action_data_repeated("$MULTICAST_LAG_ID", vec![0])
            .action_data_repeated("$DEV_PORT", TG_PIPE_PORTS_TF2.to_vec());

        switch.write_table_entry(req).await?;

        let req = Request::new("$pre.mgid")
            .match_key("$MGID", MatchValue::exact(1))
            .action_data_repeated("$MULTICAST_NODE_ID", vec![1])
            .action_data_repeated("$MULTICAST_NODE_L1_XID_VALID", vec![false])
            .action_data_repeated("$MULTICAST_NODE_L1_XID", vec![0]);

        switch.write_table_entry(req).await?;

        Ok(())
    }

    /// Deletes a simple multicast group.
    ///
    /// # Arguments
    ///
    /// * `switch`: Switch connection.
    pub async fn delete_simple_multicast_group(
        switch: &SwitchConnection,
    ) -> Result<(), RBFRTError> {
        let req = Request::new("$pre.mgid").match_key("$MGID", MatchValue::exact(1));

        let _ = switch.delete_table_entry(req).await;

        let req = Request::new("$pre.node").match_key("$MULTICAST_NODE_ID", MatchValue::exact(1));

        let _ = switch.delete_table_entry(req).await;

        Ok(())
    }

    pub async fn fill_packet_buffer(switch: &SwitchConnection) -> Result<(), RBFRTError> {
        // Create an empty 64 byte frame. The first byte of this will later contain a bit flag that indicates if this packet
        // is generated for the first time, or is a replicated multicast packet which syncs the register state
        let packet = [0u8; 64];
        let pkt_len = packet.len() as u32;

        let req = Request::new(APP_BUFFER_CFG_TF2)
            .match_key("pkt_buffer_offset", MatchValue::exact(0)) // Just write at beginning of buffer
            .match_key("pkt_buffer_size", MatchValue::exact(pkt_len)) // Minimum sized ethernet frames
            .action_data_repeated("buffer", vec![packet.to_vec()]);

        switch.update_table_entry(req).await?;

        Ok(())
    }

    /// Enable the packet generator on the tofino for all pipes
    pub async fn enable_pkt_gen(switch: &SwitchConnection) -> Result<(), RBFRTError> {
        // Activates packet generator for all pipes with the internal generation port
        let req: Vec<Request> = TG_PIPE_PORTS_TF2
            .iter()
            .copied()
            .map(|x| {
                Request::new(PORT_CFG_TF2)
                    .match_key("dev_port", MatchValue::exact(x))
                    .action_data("pktgen_enable", true)
            })
            .collect();

        switch.update_table_entries(req).await?;

        info!("Activated traffic gen capabilities.");

        Ok(())
    }
}
