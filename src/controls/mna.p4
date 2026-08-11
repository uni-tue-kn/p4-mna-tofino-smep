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

#include "mna/mna_first_nas.p4"
#include "mna/mna_second_nas.p4"

control MNA(inout header_t hdr,
            inout ingress_metadata_t ig_md,
            inout ingress_intrinsic_metadata_for_tm_t ig_tm_md,
            in ingress_intrinsic_metadata_t ig_intr_md,
            inout ingress_intrinsic_metadata_for_deparser_t ig_dprsr_md) {

    MNA_FIRST_NAS() mna_first_nas_c;
    MNA_SECOND_NAS() mna_second_nas_c;

    action drop(){
        ig_dprsr_md.drop_ctl = 0x1;
    }

    action invalidate_first_nas(){
        hdr.mna_nasi.setInvalid();

        hdr.mna_initial_opcode.setInvalid();

        hdr.mna_subsequent_opcodes[0].setInvalid();
        hdr.mna_subsequent_opcodes[1].setInvalid();
        hdr.mna_subsequent_opcodes[2].setInvalid();
        hdr.mna_subsequent_opcodes[3].setInvalid();
        hdr.mna_subsequent_opcodes[4].setInvalid();
        hdr.mna_subsequent_opcodes[5].setInvalid();
        hdr.mna_subsequent_opcodes[6].setInvalid();
        hdr.mna_subsequent_opcodes[7].setInvalid();
        hdr.mna_subsequent_opcodes[8].setInvalid();
        hdr.mna_subsequent_opcodes[9].setInvalid();
        hdr.mna_subsequent_opcodes[10].setInvalid();
        hdr.mna_subsequent_opcodes[11].setInvalid();
        hdr.mna_subsequent_opcodes[12].setInvalid();
        hdr.mna_subsequent_opcodes[13].setInvalid();
        hdr.mna_subsequent_opcodes[14].setInvalid();
    }

    action invalidate_second_nas(){
        hdr.nasi_second_nas.setInvalid();

        hdr.mna_initial_opcode_second_nas.setInvalid();

        hdr.mna_subsequent_opcodes_second_nas[0].setInvalid();
        hdr.mna_subsequent_opcodes_second_nas[1].setInvalid();
        hdr.mna_subsequent_opcodes_second_nas[2].setInvalid();
        hdr.mna_subsequent_opcodes_second_nas[3].setInvalid();
        hdr.mna_subsequent_opcodes_second_nas[4].setInvalid();
        hdr.mna_subsequent_opcodes_second_nas[5].setInvalid();
        hdr.mna_subsequent_opcodes_second_nas[6].setInvalid();
        hdr.mna_subsequent_opcodes_second_nas[7].setInvalid();
        hdr.mna_subsequent_opcodes_second_nas[8].setInvalid();
        hdr.mna_subsequent_opcodes_second_nas[9].setInvalid();
        hdr.mna_subsequent_opcodes_second_nas[10].setInvalid();
        hdr.mna_subsequent_opcodes_second_nas[11].setInvalid();
        hdr.mna_subsequent_opcodes_second_nas[12].setInvalid();
        hdr.mna_subsequent_opcodes_second_nas[13].setInvalid();
        hdr.mna_subsequent_opcodes_second_nas[14].setInvalid();
    }

    action set_resubmit(){
        ig_dprsr_md.resubmit_type = 1;
    }

    apply {

        if (ig_md.resubmit.processing_stage == 0 && hdr.mna_initial_opcode.isValid()){
            // Processing stage 0, packet seen for the first time
            mna_first_nas_c.apply(hdr, ig_md, ig_tm_md, ig_intr_md, ig_dprsr_md);
        }
        else if (ig_md.resubmit.processing_stage == 1 || (ig_md.resubmit.processing_stage == 0 && hdr.mna_initial_opcode_second_nas.isValid())){
            // Processing stage 2, either after resubmit, or if no first NAS was present
            mna_second_nas_c.apply(hdr, ig_md, ig_tm_md, ig_intr_md, ig_dprsr_md);
        }

        if (ig_md.resubmit_needed == 1){
            // This packet will be resubmitted to process the second NAS
            set_resubmit();
        } else {
            // This packet will exit the switch now

            // First NAS follows directly after the top-of-stack label
            // --> Always exposed --> pop it
            invalidate_first_nas();

            if (!hdr.mpls_inbetween_0.isValid()){
                // Invalidate Second NAS if no inbetween labels, i.e., exposed to the top
                invalidate_second_nas();

                if (ig_md.bos_reached == 1){
                    // All MNA and MPLS labels are popped, repair the ether type
                    hdr.ethernet.ether_type = ether_type_t.IPV4;
                }
            }
        };
    }
}