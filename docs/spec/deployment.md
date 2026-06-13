# 8. Deployment Profiles

| Profile | Hardware floor | Stack |
|---|---|---|
| Solo practice | 1× mini-PC + workstations | Full Postgres each machine; practice node = parent |
| Rural clinic (off-grid) | Raspberry Pi 5 class, solar | Postgres on Pi; sneakernet/3G sync to district |
| Hospital department | 1 small server | Postgres + sync service, scoped mirror |
| Hospital core | HA Postgres pair | Patroni-style failover; parent for departments |
| Regional/national | Cluster | Aggregation, registries, cross-facility matching, master patient index |

Packaging: single container image / Debian package per node; configuration declares tier, parent, sync scope. Zero-DBA target for lower tiers.
