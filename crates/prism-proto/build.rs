fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc not found");
    std::env::set_var("PROTOC", protoc);

    let proto_dir = std::path::Path::new("../../proto");
    prost_build::compile_protos(
        &[
            proto_dir.join("node_record.proto"),
            proto_dir.join("chunk_transfer.proto"),
        ],
        &[proto_dir],
    )
    .expect("prost-build failed");
}
