use std::process::Command;

fn main() {
    let bios_path = env!("BIOS_PATH");

    let mut args: Vec<String> = vec![
        "-drive".into(),
        format!("format=raw,file={bios_path}"),
        "-serial".into(),
        "stdio".into(),
        "-netdev".into(),
        "user,id=net0,hostfwd=tcp::8080-:80".into(),
        "-device".into(),
        "e1000,netdev=net0".into(),
        "-device".into(),
        "isa-debug-exit,iobase=0xf4,iosize=0x04".into(),
        "--no-reboot".into(),
    ];
    args.extend(std::env::args().skip(1));

    println!("Booting BIOS image: {bios_path}");
    let status = Command::new("qemu-system-x86_64")
        .args(&args)
        .status()
        .expect("failed to run qemu-system-x86_64 (is QEMU installed?)");

    std::process::exit(status.code().unwrap_or(0));
}
