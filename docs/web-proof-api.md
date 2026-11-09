# Web proof API — contract only (v1.1)

No server ships in v0.1. These types (`crates/oto-proof/src/api.rs`) fix the wire format so that a future server and the desktop app agree.

| Method | Path | Purpose |
|---|---|---|
| POST | /v1/proofs | Create a proof (metadata only, no photos) |
| POST | /v1/proofs/{id}/revisions | Upload a revision (spread JPEGs + manifest) |
| GET | /v1/proofs/{id} | Proof + latest revision + comments |
| POST | /v1/proofs/{id}/comments | Add an anchored comment (spread, x/y, photo) |
| POST | /v1/proofs/{id}/approve | Client approval bound to one revision |

Security: clients use an unguessable share token plus an optional PIN. PINs are stored only as Argon2id hashes on the server, and nothing is stored in plaintext. Photos are uploaded only when the photographer creates a revision.
