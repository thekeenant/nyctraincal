fn main() {
    println!("cargo:rerun-if-changed=proto/com/google/transit/realtime/gtfs-realtime.proto");
    println!(
        "cargo:rerun-if-changed=proto/com/google/transit/realtime/gtfs-realtime-service-status.proto"
    );
    println!("cargo:rerun-if-changed=proto/com/google/transit/realtime/gtfs-realtime-NYCT.proto");

    protobuf_codegen::Codegen::new()
        .pure()
        .includes(["proto"])
        .input("proto/com/google/transit/realtime/gtfs-realtime.proto")
        .input("proto/com/google/transit/realtime/gtfs-realtime-service-status.proto")
        .input("proto/com/google/transit/realtime/gtfs-realtime-NYCT.proto")
        .cargo_out_dir("proto")
        .run_from_script();
}
