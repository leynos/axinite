//! Minimal panic and unwinding cases for comparing Rust codegen backends.

#[cfg(test)]
mod panic_backend {
    use std::panic::catch_unwind;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn catch_unwind_catches() {
        let result = catch_unwind(|| {
            if std::hint::black_box(true) {
                panic!("boom");
            }
        });

        assert!(result.is_err());
    }

    #[test]
    fn spawned_thread_panic_is_join_error() {
        let join = std::thread::spawn(|| {
            if std::hint::black_box(true) {
                panic!("boom");
            }
        });

        assert!(join.join().is_err());
    }

    #[test]
    #[should_panic(expected = "boom")]
    fn should_panic_works() {
        if std::hint::black_box(true) {
            panic!("boom");
        }
    }

    #[test]
    fn nested_drop_runs_during_unwind() {
        struct DropMarker(Arc<AtomicBool>);

        impl Drop for DropMarker {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
                eprintln!("drop marker ran during unwind");
            }
        }

        let dropped = Arc::new(AtomicBool::new(false));
        let caught = catch_unwind({
            let dropped = Arc::clone(&dropped);
            move || {
                let _marker = DropMarker(dropped);
                if std::hint::black_box(true) {
                    panic!("boom");
                }
            }
        });

        assert!(caught.is_err());
        assert!(dropped.load(Ordering::SeqCst));
    }
}
