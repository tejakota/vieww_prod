//! One module per screen.
//!
//! Each screen exposes `pub fn screen() -> impl Widget`, which is the contract
//! vieww Studio's preview looks for: open the file, press Render, and it
//! appears in the device frame without the application having to run.

pub mod home;
