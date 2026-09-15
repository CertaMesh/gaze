//! Unchanged independent proof, including exact-base and strict staged controls.
#[rustfmt::skip]
mod original { include!("fixtures/recovery7835/collision-regression.rs"); }

#[rustfmt::skip]
#[path = "fixtures/recovery7835/accepted-collision-base.rs"]
mod accepted_base;
