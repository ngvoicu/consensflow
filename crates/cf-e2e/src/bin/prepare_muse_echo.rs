//! Put the pinned echo-test Muse on a disposable GitHub runner, without a login.

use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

const VERSION: &str = "1.4.14-R7309.1";

fn artifact() -> Result<(&'static str, &'static str, u64), Box<dyn std::error::Error>> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok((
            "muse-aarch64-macos",
            "1c96114408acbeade9a8fb79e3b4425e5b64d50425938568aef01bdc04ce2e03",
            423_687_488,
        )),
        ("macos", "x86_64") => Ok((
            "muse-x86-macos",
            "6da5a12d3298afba45afec929fc385b516084ed28e9504ae29354dbcdfc4089d",
            443_142_560,
        )),
        ("windows", "x86_64") => Ok((
            "muse-x86-windows.exe",
            "20d18c426cc26e9e4b3cc07008ec07ec97e12e466a2be8cd2797b2e0e2018382",
            435_429_624,
        )),
        ("windows", "aarch64") => Ok((
            "muse-aarch64-windows.exe",
            "64cef76cb210de993d31478ee582ef4639bbb212df8912b74f45d40431e66e82",
            398_733_560,
        )),
        _ => Err("Muse echo has no pinned artifact for this platform".into()),
    }
}

#[allow(clippy::print_stdout)] // The hosted job names the executable it prepared.
fn prepare(folder: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let (name, expected, size) = artifact()?;
    let url = format!("https://lookaside.facebook.com/lookaside/muse/download/?channel=muse&version={VERSION}&file={name}");
    let mut response = ureq::get(&url).call()?;
    let mut input = response.body_mut().as_reader().take(size + 1);
    let binary = folder.join(format!(
        "muse-bin-{VERSION}{}",
        std::env::consts::EXE_SUFFIX
    ));
    let mut output = cf_e2e::door::create_new(&binary)?;
    let mut hash = Sha256::new();
    let mut count = 0_u64;
    let mut buffer = [0_u8; 65_536];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        count += read as u64;
        hash.update(&buffer[..read]);
        output.write_all(&buffer[..read])?;
    }
    drop(output);
    if count != size || format!("{:x}", hash.finalize()) != expected {
        return Err("the pinned Muse artifact failed its length or SHA-256 check".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        cf_e2e::door::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))?;
        cf_e2e::door::write_executable(
            folder.join("muse"),
            format!("#!/bin/sh\nexec \"$(dirname \"$0\")/muse-bin-{VERSION}\" \"$@\"\n"),
        )?;
    }
    #[cfg(windows)]
    cf_e2e::door::write(
        folder.join("muse.cmd"),
        format!("@\"%~dp0muse-bin-{VERSION}.exe\" %*\r\n"),
    )?;
    cf_e2e::door::write(folder.join(".muse-version"), VERSION)?;
    println!("prepared Muse echo {VERSION} at {}", binary.display());
    Ok(())
}

#[allow(clippy::disallowed_methods)] // Read the hosted-runner markers once at startup.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("CF_E2E_HOSTED_MUSE").as_deref() != Ok("1")
    {
        return Err("Muse echo preparation requires a disposable GitHub hosted runner".into());
    }
    let folder = std::path::PathBuf::from(std::env::var_os("RUNNER_TEMP").ok_or("no RUNNER_TEMP")?)
        .join("muse-echo");
    cf_e2e::door::made_dir(&folder)?;
    if let Err(error) = prepare(&folder) {
        let _ = cf_e2e::door::Door::of_run().remove_made(&folder);
        return Err(error);
    }
    Ok(())
}
