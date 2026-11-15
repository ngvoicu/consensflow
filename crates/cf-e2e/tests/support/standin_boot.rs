//! Establish a stand-in process's homes before libtest starts its threads.

ctor::declarative::ctor! {
    #[ctor(unsafe)]
    fn begin_stand_in_process() {
        cf_e2e::standin_process::start(crate::ALLOW_LIVE_MATRIX);
    }
}
