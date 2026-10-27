# block 0.1.6 compatibility patch

Source: the crates.io block 0.1.6 archive, authored by Steven Sheldon,
https://github.com/SSheldon/rust-block (MIT, as declared by upstream Cargo.toml).
The upstream repository and registry archive do not contain a separate license file.
Upstream README and source are retained.

The empty enum used for the opaque Objective-C Class is replaced with an inhabited
repr(C) opaque struct. Only the address of _NSConcreteStackBlock is used; the
foreign storage is never read as a Rust value. Legacy implicit C ABIs are also
spelled explicitly as extern "C". This fixes Rust's
uninhabited_static future-incompatibility without changing the public API or block ABI.

The registry archive omits the Objective-C unit-test fixture. Its unavailable dev
dependency is removed and the upstream unit-test target disabled; the local
integration smoke test checks block invocation and heap copying instead.

Remove this patch when Cocoa/Metal no longer depend on block 0.1.6 or a compatible
upstream fix is available. This local patch must be accounted for when packaging
xgpui for external consumers (#49).
