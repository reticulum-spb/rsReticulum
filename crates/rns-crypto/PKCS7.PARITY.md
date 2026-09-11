# PKCS7 compatibility

Reference: Python Reticulum 1.5.2, commit
`ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`, `RNS/Cryptography/PKCS7.py`.

The strict Rust `pkcs7::unpad` behaviour is intentional and unchanged.
Valid Python-padded inputs produce identical bytes and plaintext in Rust,
including empty plaintext, every padding length from 1 to 16, and full-block
padding for aligned plaintext.

Rust rejects zero padding, padding longer than the input, and inconsistent
padding bytes. Python accepts these: zero retains the whole input, mismatched
bytes are not inspected, and a length exceeding the input uses Python slicing.
For example, four bytes of value 5 become three bytes; four bytes of value 10
become empty. Both reject a final byte above 16. Empty input fails in Python
with IndexError and in Rust with the typed EmptyInput error.

This helper validates the padding suffix, not AES block alignment: both accept
`41 01` and return `41`. The token layer checks ciphertext block alignment and
verifies HMAC before decryption and unpadding; Rust maps padding failures to
AuthenticationFailed there. No permissive unpad or decrypt API is introduced.
Accepting malformed padding is unnecessary for valid reference ciphertexts.

Cross-implementation checks live in `../reticulum-e2e-tests`:
`spec/rust/python/pkcs7_reference.py` records 43 actual Python pad/unpad results;
`pkcs7_unpad_validates_every_padding_byte` in
`spec/rust/tests/reference_contract.rs` checks valid parity and intentional
rejections against those results. Existing crypto vectors and real bidirectional
Python/Rust encrypted exchanges cover valid decryption. RUST-S3 remains in
`spec/rust/known_gaps.toml` as an intentional difference.
