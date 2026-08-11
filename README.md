<div align="center">
<h2>SMEP: Stateless MNA-Based Egress Protection in P4</h2>

![image](https://img.shields.io/badge/licence-Apache%202.0-blue) ![image](https://img.shields.io/badge/lang-rust-darkred) ![image](https://img.shields.io/badge/built%20with-P4-orange) [![Controller build](https://github.com/uni-tue-kn/p4-mna-tofino-smep/actions/workflows/controller.yml/badge.svg)](https://github.com/uni-tue-kn/p4-mna-tofino-smep/actions/workflows/controller.yml) [![Data Plane Build](https://github.com/uni-tue-kn/p4-mna-tofino-smep/actions/workflows/data_plane.yml/badge.svg)](https://github.com/uni-tue-kn/p4-mna-tofino-smep/actions/workflows/data_plane.yml)

</div>

- [Overview](#overview)
- [Installation \& Start Instructions](#installation--start-instructions)
  - [Data Plane](#data-plane)
  - [Control Plane](#control-plane)
- [Testbed Topology](#testbed-topology)
- [Evaluation](#evaluation)
- [Citing SMEP](#citing-smep)
- [References](#references)

## Overview

This repository contains the P4 implementation of Stateless MNA-based Egress Protection (SMEP) for the Intel Tofino 2 switching ASIC.
It extends [P4-MNA](https://github.com/uni-tue-kn/P4-MNA), the P4 implementation of the MPLS Network Actions (MNA) framework of the [MPLS Working Group](https://datatracker.ietf.org/wg/mpls/about/).

With SMEP, the bypass path of the MPLS egress protection framework travels in the packet instead of being installed at the point of local repair (PLR).
The ingress router pushes one or more bypass MPLS labels (BMLs) together with a network action that controls whether they take effect.
While the egress next hop is reachable, the PLR removes the BMLs.
After a failure, it leaves them in place so that they steer the packet to the protector.

The implementation features:

- **Conditional `POP-N`.** The select-scoped stack management operation removes the labels below the network action sub-stack (NAS) only while the egress-facing port is up.
- **Bypass paths of up to four BMLs**, i.e., up to four segments towards the protector.
- **Single-pass processing without recirculation.** A match-action table is applied at most once per pipeline pass, so a separate SMEP lookup table holds forwarding entries for the BMLs only.
- **Data-plane failure detection.** The port-down hardware trigger generates an event packet that updates a port liveness register and is replicated to all four pipes. A digest informs the controller afterwards, so the switchover itself does not involve the control plane.

The Rust control plane is built on [rbfrt](https://github.com/uni-tue-kn/rbfrt).

## Installation & Start Instructions

### Data Plane

Compile SMEP, which also copies the resulting configs to the target directory:

```bash
make compile TARGET=tofino2
```

Afterwards, start SMEP:

```bash
make start TARGET=tofino2
```

This requires a fully setup SDE with set `$SDE` and `$SDE_INSTALL` environment variables.

### Control Plane

The controller is written in Rust. Build and start it via:

```bash
cd Local-Controller && cargo run
```

It sets up the MPLS forwarding entries, the `POP-N` opcode entries, and the port-down trigger for the topology below.
Adapt the port numbers and MPLS labels to your device; they are defined as constants at the top of `run()` in `Local-Controller/src/main.rs`.

Port configuration is skipped by default via the `CONFIGURE_PORTS` constant.
Enable it if the ports have not been configured externally.

## Testbed Topology

All roles are hosted on a single Intel Tofino 2.
A second Tofino 2 runs (v2.8.0) and emulates the ingress LER and the customer node.

```
                    device under test (one Tofino 2)

  P4TG  ---100G--->  p.13-16                        ingress LER
                        |
                        |  label 100
                        v
                       p.4                          PLR
                        |
       egress up        |        egress down
       label 200        |        labels 201, 202
       +----------------+-----------------+
       |                                  |
       v                                  v
     p.11  ---400G cable--->  p.12      p.5  --->  p.6
     protected link           egress    bypass 1   bypass 2
       |                                  |        (protector)
       +---------  labels 300-303  -------+
                        |
                        v
                     p.13-16  ---100G--->  P4TG
```

| Role                  | Port           | Speed | Type                       |
| --------------------- | -------------- | ----- | -------------------------- |
| P4TG streams          | 13, 14, 15, 16 | 100G  | cabled                     |
| PLR                   | 4              | 400G  | internal MAC-near loopback |
| Protected egress link | 11 → 12        | 400G  | cabled loopback            |
| First bypass hop      | 5              | 400G  | internal MAC-near loopback |
| Second bypass hop     | 6              | 400G  | internal MAC-near loopback |

Labels are assigned as follows:

| Label    | Meaning                                                |
| -------- | ------------------------------------------------------ |
| 100      | to the PLR                                             |
| 200      | to the egress over the protected link                  |
| 201, 202 | the two BMLs of the bypass tunnel                      |
| 300-303  | delivery labels returning each stream to its P4TG port |

P4TG pushes the labels for the PLR and the egress, the select-scoped `POP-N` for two BMLs, both BMLs, and a delivery label that returns the packet.
Each re-entry through a loopback performs one label switching operation.

The protected egress link is a physical 400G cable between ports 11 and 12, since only a real link raises the PHY port-down event that the trigger detects.
Port 11 sits on a different pipe than the PLR on port 4, which exercises the cross-pipe distribution of the port-down event.
A failure is injected by administratively disabling port 11 or by pulling the transceiver.

## Evaluation

`p4tg_smep.json` contains the stream configuration for the traffic generator [P4TG](https://github.com/uni-tue-kn/P4TG) that was used for the evaluation in the paper.

## Citing SMEP

If you use SMEP in any of your publications, please cite the following paper:

- TBA

## References

The mechanism and the underlying stack management operation are specified in individual Internet-Drafts:

- [draft-ihle-mpls-mna-stateless-egress-protection](https://datatracker.ietf.org/doc/draft-ihle-mpls-mna-stateless-egress-protection/): Stateless MNA-based Egress Protection (SMEP)
- [draft-ihle-mpls-mna-stack-management](https://datatracker.ietf.org/doc/draft-ihle-mpls-mna-stack-management/): MPLS Network Action for Stack Management

This implementation builds on P4-MNA:

- [F. Ihle and M. Menth: MPLS Network Actions: Technological Overview and P4-Based Implementation on a High-Speed Switching ASIC](https://ieeexplore.ieee.org/document/10947349), [erratum](https://ieeexplore.ieee.org/document/11010923), [preprint](https://atlas.cs.uni-tuebingen.de/~ihle/IhMe24.pdf), in IEEE Open Journal of the Communications Society, vol. 6, pp. 3480 - 3501, 2025, IEEE
- [P4-MNA repository](https://github.com/uni-tue-kn/P4-MNA)
