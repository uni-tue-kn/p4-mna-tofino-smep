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

#include "mna.p4"

control MPLS(inout header_t hdr, 
            inout ingress_metadata_t ig_md, 
            inout ingress_intrinsic_metadata_for_tm_t ig_tm_md, 
            in ingress_intrinsic_metadata_t ig_intr_md, 
            inout ingress_intrinsic_metadata_for_deparser_t ig_dprsr_md) {

    MNA() mna_c;

    DirectCounter<bit<32>>(CounterType_t.PACKETS) debug_smep_counter;
    DirectCounter<bit<32>>(CounterType_t.PACKETS) debug_mpls_counter;
    DirectCounter<bit<32>>(CounterType_t.PACKETS) debug_pop_smep_labels_counter;

    /*
    This register keeps track of the status of each port (up or down).
    The index is the dev port number. If it is set to 1, the port is down.
    If it is set to 0, the port is up.
    */
    // TODO sync register state across all pipes
    Register<bit<8>, PortId_t>(512, 0) port_status;
    RegisterAction<bit<8>, PortId_t, bit<8>>(port_status) get_port_down_status = {
            void apply(inout bit<8> value, out bit<8> read_value) {
                read_value = value;
            }
    };
    RegisterAction<bit<8>, PortId_t, void>(port_status) set_port_down = {
            void apply(inout bit<8> value) {
                value = 1;
            }
    };
    RegisterAction<bit<8>, PortId_t, void>(port_status) set_port_up = {
            void apply(inout bit<8> value) {
                value = 0;
            }
    };

    action drop(){
        ig_dprsr_md.drop_ctl = 0x1;
    }

    action nothing(){
        debug_mpls_counter.count();
    } 

    action forward(PortId_t port){
        ig_tm_md.ucast_egress_port = port;
        hdr.mpls.ttl = hdr.mpls.ttl - 1;

        debug_mpls_counter.count();
    }

    action forward_and_pop(PortId_t port){
        ig_tm_md.ucast_egress_port = port;
        hdr.mpls.ttl = hdr.mpls.ttl - 1;
        hdr.mpls.setInvalid();

        debug_mpls_counter.count();
    }

    action forward_and_pop_last_label(PortId_t port){
        // Additionally rewrites the ethertype and copies TTL
        ig_tm_md.ucast_egress_port = port;
        hdr.mpls.ttl = hdr.mpls.ttl - 1;
        hdr.ethernet.ether_type = ether_type_t.IPV4;
        hdr.ipv4.ttl = hdr.mpls.ttl;
        hdr.mpls.setInvalid();

        debug_mpls_counter.count();
    }    

    action forward_and_swap(PortId_t port, bit<20> label){
        ig_tm_md.ucast_egress_port = port;
        hdr.mpls.ttl = hdr.mpls.ttl - 1;
        hdr.mpls.label = label;

        debug_mpls_counter.count();
    }    

    table mpls_lookup_table {
        key = {
            hdr.mpls.label: exact;
            ig_md.resubmit_needed: exact;
        }
        actions = {
            forward;
            forward_and_swap;
            forward_and_pop;
            forward_and_pop_last_label;
            nothing;
        }
        default_action = nothing;
        size = 1024;
        counters = debug_mpls_counter;
    }

    action smep_forward(PortId_t port){
        ig_tm_md.ucast_egress_port = port;
        debug_smep_counter.count();
    }

    action smep_forward_and_swap(PortId_t port, bit<20> label){
        ig_tm_md.ucast_egress_port = port;
        hdr.mpls_inbetween_0.label = label;
        debug_smep_counter.count();
    }    

    action smep_forward_and_pop(PortId_t port){
        ig_tm_md.ucast_egress_port = port;
        hdr.mpls_inbetween_0.setInvalid();
        debug_smep_counter.count();
    }

    table mpls_smep_lookup_table {
        key = {
            hdr.mpls_inbetween_0.label: exact;
            ig_md.resubmit_needed: exact;
        }
        actions = {
            smep_forward_and_pop;
            smep_forward;
            smep_forward_and_swap;
        }
        size = 1024;
        counters = debug_smep_counter;
    }    

    table verify_ttl {
        key = {
            hdr.mpls.ttl: exact;
        }
        actions = {
            drop;
            NoAction;
        }
        default_action = NoAction;
        size = 256;
    }



    action pop_1_label() {
        debug_pop_smep_labels_counter.count();
        ig_md.popped_bos = hdr.mpls_inbetween_0.bos;
        hdr.mpls_inbetween_0.setInvalid();
    }
    action pop_2_label() {
        debug_pop_smep_labels_counter.count();
        ig_md.popped_bos = hdr.mpls_inbetween_1.bos;
        hdr.mpls_inbetween_0.setInvalid();
        hdr.mpls_inbetween_1.setInvalid();
    }
    action pop_3_label() {
        debug_pop_smep_labels_counter.count();
        ig_md.popped_bos = hdr.mpls_inbetween_2.bos;
        hdr.mpls_inbetween_0.setInvalid();
        hdr.mpls_inbetween_1.setInvalid();
        hdr.mpls_inbetween_2.setInvalid();
    }
    action pop_4_label() {
        debug_pop_smep_labels_counter.count();
        hdr.mpls_inbetween_0.setInvalid();
        hdr.mpls_inbetween_1.setInvalid();
        hdr.mpls_inbetween_2.setInvalid();
        hdr.mpls_inbetween_3.setInvalid();
    }

    action nop() {
        debug_pop_smep_labels_counter.count();
    }

    table pop_smep_labels {
        key = {
            ig_md.smep.pop_labels: exact;
        }
        actions = {
            pop_1_label;
            pop_2_label;
            pop_3_label;
            pop_4_label;
            nop;
        }
        size = 16;
        default_action = nop();
        counters = debug_pop_smep_labels_counter;
    }

    apply {
        mna_c.apply(hdr, ig_md, ig_tm_md, ig_intr_md, ig_dprsr_md);

        if (hdr.mpls.isValid()) {
            //verify_ttl.apply();
            // Lookup egress port + Pop/Swap label
            mpls_lookup_table.apply();

            ig_md.smep.egress_port_status = get_port_down_status.execute(ig_tm_md.ucast_egress_port);
            // TODO only do this for egress nodes / PLR
            if (ig_md.smep.egress_port_status == 1) {
                // Egress port is down :( Do egress port rewrite
                mpls_smep_lookup_table.apply();
            } else {
                // Apply SMEP POP-N logic
                pop_smep_labels.apply();
            }
            if (ig_md.popped_bos == 1) {
                // Last label we popped was BoS, repair the ether type
                hdr.ethernet.ether_type = ether_type_t.IPV4;
            }
        } else if (hdr.port_down.isValid()) {
            set_port_down.execute(hdr.port_down.port_num);
            // Generate a digest that tells the control plane that a port went down
            ig_dprsr_md.digest_type = 1;

            // Register<->pipe sync mechanism
            if (hdr.pktgen_port_down_multicast.is_replicated == 0) {
                ig_tm_md.mcast_grp_a = 1; // Replicate such a packet to all pipes
                hdr.pktgen_port_down_multicast.is_replicated = 1;
            }
        }
    }
}