{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
	packages = with pkgs; [
		cargo
		cargo-auditable
		clippy
		rustc
		rustfmt
		pkg-config
		openssl
	];

	RUST_BACKTRACE = "1";
}
