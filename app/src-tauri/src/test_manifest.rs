//! The manifest the library's test binary needs on Windows.
//!
//! The dialog code the library links imports `TaskDialogIndirect`, which only
//! Common Controls 6 exports, and a program asks for that version through its
//! manifest. The app has Tauri's, linked into its own binary alone. Cargo gives
//! a test binary none, so the library's tests loaded the old comctl32 and never
//! started (`STATUS_ENTRYPOINT_NOT_FOUND`). The manifest is linked into that
//! binary, and into no other:
//!
//! - `build.rs` writes `tests.manifest` as a resource file, `tests_manifest.lib`
//!   in `OUT_DIR` (`build/res.rs` has the format), when it builds for Windows
//!   with MSVC, and tells Cargo to search there. MSVC's linker takes a `.res` by
//!   what is in the file and not by its name, which `embed-resource` relies on
//!   for the resources it makes.
//! - The declaration below, compiled for the library's tests alone, links it.
//!
//! Cargo's `rustc-link-arg-tests` will not do, nor will `embed-resource`'s
//! `compile_for_tests`, which prints it: they reach the tests under `tests/` and
//! not the library's own (seen on Cargo 1.99.0). A link argument for every
//! target would reach the app too, and two manifests in one binary fail its
//! link.
//!
//! The kind is `dylib`: rustc hands the linker `tests_manifest.lib` and never
//! opens it. `static` is the kind rustc bundles into the libraries it writes, by
//! reading the file as an archive, and a `.res` is not one.
//!
//! An integration test under `tests/` would link the library without `cfg(test)`
//! and get none of this: it would have to link `tests_manifest` itself.

// Also compiled into the build script, which writes the resource (see its docs).
#[path = "../build/res.rs"]
mod res;

#[cfg(all(windows, target_env = "msvc"))]
#[link(name = "tests_manifest", kind = "dylib")]
unsafe extern "C" {}

/// Reads the manifest back from the running test binary, as the loader does.
#[cfg(all(windows, target_env = "msvc"))]
mod embedded {
    use std::ptr::{self, NonNull};

    use windows_sys::Win32::System::LibraryLoader::{
        FindResourceW, GetModuleHandleW, LoadResource, LockResource, SizeofResource,
    };

    use super::res::{EXECUTABLE_MANIFEST, RT_MANIFEST};

    /// The running executable's manifest, if it has one.
    fn own_manifest() -> Option<&'static [u8]> {
        // MAKEINTRESOURCE: a number where the address of a name goes.
        let number = |number: u16| ptr::without_provenance::<u16>(usize::from(number));
        // SAFETY: the calls read this process's own image, which stays mapped as
        // long as the process runs, so the bytes outlive any borrow of them; a
        // handle is used only when the call before it gave one.
        unsafe {
            let module = GetModuleHandleW(ptr::null());
            let found = NonNull::new(FindResourceW(
                module,
                number(EXECUTABLE_MANIFEST),
                number(RT_MANIFEST),
            ))?;
            let loaded = NonNull::new(LoadResource(module, found.as_ptr()))?;
            let data = NonNull::new(LockResource(loaded.as_ptr()))?;
            let size = usize::try_from(SizeofResource(module, found.as_ptr())).ok()?;
            Some(std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), size))
        }
    }

    #[test]
    fn the_test_binary_carries_the_common_controls_6_manifest() {
        let embedded = own_manifest()
            .expect("the test binary has no manifest: tests_manifest.lib was not linked");
        let embedded = std::str::from_utf8(embedded).expect("a manifest is text");
        // The file itself, whole: nothing cut off, and none of the padding.
        assert_eq!(embedded, include_str!("../tests.manifest"));
        let identity = embedded
            .split("<assemblyIdentity")
            .nth(1)
            .and_then(|rest| rest.split("/>").next())
            .expect("the manifest names the assembly it depends on");
        assert!(
            identity.contains(r#"name="Microsoft.Windows.Common-Controls""#),
            "{identity}"
        );
        assert!(identity.contains(r#"version="6.0.0.0""#), "{identity}");
    }
}
