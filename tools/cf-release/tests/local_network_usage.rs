//! The app explains to the person why its agents may need the local network.

#[test]
fn the_app_explains_its_local_network_use() {
    let source = include_bytes!("../../../app/src-tauri/Info.plist");
    let plist = plist::Value::from_reader_xml(source.as_slice()).expect("valid app Info.plist");
    let description = plist
        .as_dictionary()
        .and_then(|dict| dict.get("NSLocalNetworkUsageDescription"))
        .and_then(plist::Value::as_string);
    assert_eq!(
        description,
        Some("ConsensFlow's agents connect to model providers and tools on your local network.")
    );
}
