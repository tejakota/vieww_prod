// The process's entry point, and why it is Objective-C.
//
// `UIApplicationMain` has to own the process before any window exists, and it
// never returns — so on iOS the `main` that runs is not Rust's. The Rust side
// is linked in as a static library and exposes one symbol; this calls it, and
// `vieww_ios_main` hands control to winit, which calls `UIApplicationMain`
// itself.
//
// Declared here rather than in a generated header because it is one function
// with no arguments, and a header for it would be a file to keep in sync with
// `src/lib.rs` for no benefit.

extern void vieww_ios_main(void);

int main(int argc, char *argv[]) {
    (void)argc;
    (void)argv;
    vieww_ios_main();
    return 0;
}
