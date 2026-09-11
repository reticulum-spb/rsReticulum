# Token length compatibility

Reference: Python Reticulum 1.5.2, commit
`ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`, `RNS/Cryptography/Token.py`.

Rust's 64-byte minimum is intentional and unchanged: a valid token needs a
16-byte IV, at least one 16-byte AES-CBC block containing PKCS7 padding,
and a 32-byte HMAC. Empty plaintext still produces that full padding block.
This applies to both supported key lengths (32-byte AES128 and 64-byte AES256).

Python rejects lengths <=32 at its HMAC length gate. At lengths 33 and 63 it
checks HMAC: an invalid tag fails there, while a matching tag reaches decrypt
and still fails. A matching tag alone does not make a valid token. Rust rejects
all these short tokens before HMAC, with AuthenticationFailed. At 64 bytes,
both reject malformed tokens; valid minimal tokens decrypt successfully.
Rust deliberately keeps one authentication failure result for malformed tokens.
The public error does not reveal which check failed; the early threshold is
verified by inspection of token::decrypt, not inferred from its error value.
No length limit is lowered and no permissive decrypt path is introduced.

Cross-implementation checks live in `../reticulum-e2e-tests`:
`spec/rust/python/token_length_reference.py` calls real Python verify_hmac and
decrypt on 16 fixed cases (lengths 32/33/63/64, correct/incorrect tags, both key
lengths). `token_decrypt_rejects_everything_below_sixty_four_bytes` mirrors
rejection through Rust's public decrypt API. The vector exchange producers
also encrypt empty plaintext with real APIs in both key modes; the other
implementation checks the 64-byte token and decrypts it. These tokens retain
random IVs and are generated per run, never committed as run artifacts.
Existing non-empty plaintext exchanges remain covered. RUST-S4 remains in
the gap registry as an intentional rejection-stage/error difference.
