fn main() {
    let proto_file = "./proto/backends.proto";

    println!("building proto {}", proto_file);

    tonic_build::configure()
        .protoc_arg("--experimental_allow_proto3_optional")
        .out_dir("./src")
        .compile(&[proto_file], &["."])
        .unwrap();
}
