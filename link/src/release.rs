//! Distribution identity is independent of wire protocol admission.
#[used]
static RELEASE: [u8; include_bytes!(concat!(env!("OUT_DIR"), "/release.bin")).len()] =
    *include_bytes!(concat!(env!("OUT_DIR"), "/release.bin"));

pub fn identity() -> serde_json::Value {
    serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/release.json"))).unwrap()
}

pub fn print_version(component: &str) -> bool {
    if std::env::args_os().skip(1).collect::<Vec<_>>() != ["--version-json"] {
        return false;
    }
    std::hint::black_box(&RELEASE);
    let mut value = identity();
    value["component"] = component.into();
    println!("{value}");
    true
}
