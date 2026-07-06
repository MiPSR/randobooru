{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
	packages = with pkgs; [
		cargo
		cargo-auditable
		clippy
		rustc
		rustfmt
	];

	RUST_BACKTRACE = "1";
}
