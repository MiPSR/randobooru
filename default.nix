{ pkgs ? import <nixpkgs> {} }:

pkgs.pkgsStatic.rustPlatform.buildRustPackage {
	pname = "randobooru";
	version = "26.1.1-alpha";
	src = ./.;

	cargoLock.lockFile = ./Cargo.lock;

	RUSTFLAGS = "-C target-feature=+crt-static";

	meta = with pkgs.lib; {
		description = "Discord bot that returns random booru images from configurable tag presets";
		mainProgram = "randobooru";
		platforms = platforms.linux;
	};
}
