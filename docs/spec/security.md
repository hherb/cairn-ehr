# 7. Security & Compliance (macroscopic)

- **Encryption at rest** mandatory below facility tier (LUKS + per-database encryption).
- **Offline authentication:** cached short-lived credentials/certificates per device and user; offline access automatically narrows; break-glass with mandatory retrospective audit.
- **Audit log is an event stream**, syncing upstream at highest priority.
- mTLS between nodes; enrollment via explicit trust/provisioning ceremony (also regenerates machine identity and PRNG seed — see [data-model §3.2](data-model.md#32-identity-time)).
- **Visibility scopes on link events** ([§5.6](identity.md#56-pseudonymous-sanctioned-care)): access-control and identity-linkage decisions are coupled by design.
- Compliance posture (GDPR/HIPAA/national law) is configuration; core guarantees (encryption, audit, access control) are universal.
