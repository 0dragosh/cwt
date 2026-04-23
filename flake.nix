{
  description = "cwt — Claude Worktree Manager";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, rust-overlay, crane, ... }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      verusVersion = "0.2026.04.19.6f7d4de";
      verusSources = {
        aarch64-darwin = {
          asset = "verus-${verusVersion}-arm64-macos.zip";
          hash = "sha256-RMv+8CiyJ66XFN2nJ8QZnuYLakIQXJ8PIH47BQbRCxY=";
          rustToolchain = "1.95.0-aarch64-apple-darwin";
        };
        x86_64-darwin = {
          asset = "verus-${verusVersion}-x86-macos.zip";
          hash = "sha256-Dfw54tQcwfmkZpJHgy5Hry/hL3TM9oRK/wKyJ+ZaAvc=";
          rustToolchain = "1.95.0-x86_64-apple-darwin";
        };
        x86_64-linux = {
          asset = "verus-${verusVersion}-x86-linux.zip";
          hash = "sha256-cChaGWIBB9HYez/Ef1DFshhP5AVDe5hqPzEGuE/rdKU=";
          rustToolchain = "1.95.0-x86_64-unknown-linux-gnu";
        };
      };
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          rustToolchain = pkgs.rust-bin.stable.latest.default;
          craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              let
                relPath = pkgs.lib.removePrefix "${toString ./.}/" (toString path);
              in
              craneLib.filterCargoSources path type
              # Keep the verification sidecar alongside Cargo sources so Nix
              # test builds see the same workflow artifacts as local runs.
              || relPath == "verification"
              || pkgs.lib.hasPrefix "verification/" relPath
              || relPath == "scripts"
              || relPath == "scripts/verify-verus.sh"
              || relPath == "docs"
              || relPath == "docs/verification.md"
              || relPath == ".github"
              || relPath == ".github/workflows"
              || pkgs.lib.hasPrefix ".github/workflows/" relPath;
          };

          cwt = craneLib.buildPackage {
            pname = "cwt";
            version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;
            inherit src;

            nativeBuildInputs = with pkgs; [ pkg-config makeWrapper git ];
            buildInputs = with pkgs; [ ]
              ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
                pkgs.apple-sdk_15
              ];

            # Integration tests need git with a configured identity
            preCheck = ''
              export HOME=$(mktemp -d)
              git config --global user.email "test@cwt.dev"
              git config --global user.name "cwt-test"
              git config --global init.defaultBranch main
            '';

            # Runtime deps that should be on PATH
            postInstall = ''
              wrapProgram $out/bin/cwt \
                --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.git pkgs.tmux ]}
            '';
          };
        in
        {
          default = cwt;
          cwt = cwt;
        }
      );

      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" "rust-analyzer" "clippy" ];
          };
          verusMeta = verusSources.${system} or null;
          verus = if verusMeta == null then null else pkgs.stdenvNoCC.mkDerivation {
            pname = "verus";
            version = verusVersion;

            src = pkgs.fetchurl {
              url = "https://github.com/verus-lang/verus/releases/download/release/${verusVersion}/${verusMeta.asset}";
              hash = verusMeta.hash;
            };

            nativeBuildInputs = with pkgs; [ unzip makeWrapper ];
            dontBuild = true;
            unpackPhase = ''
              runHook preUnpack
              mkdir source
              unzip -q "$src" -d source
              cd source
              runHook postUnpack
            '';

            installPhase = ''
              runHook preInstall

              mkdir -p "$out/lib/verus" "$out/bin"
              release_root=
              for dir in */; do
                release_root="$dir"
                break
              done
              if [ -z "$release_root" ]; then
                echo "Verus release zip did not contain a top-level directory" >&2
                exit 1
              fi
              cp -R "$release_root"/. "$out/lib/verus"
              chmod -R u+w "$out/lib/verus"
              chmod +x "$out/lib/verus/verus" "$out/lib/verus/cargo-verus" \
                "$out/lib/verus/rust_verify" "$out/lib/verus/z3"
              makeWrapper "$out/lib/verus/verus" "$out/bin/verus" \
                --run "cd $out/lib/verus"
              makeWrapper "$out/lib/verus/cargo-verus" "$out/bin/cargo-verus" \
                --run "cd $out/lib/verus"

              runHook postInstall
            '';
          };
          verusShell = pkgs.mkShell {
            buildInputs = with pkgs; [
              rustup
              unzip
              verus
            ];

            shellHook = ''
              echo "cwt Verus shell - Verus ${verusVersion}"
              echo "  verify: ./scripts/verify-verus.sh"
              if ! rustup run ${verusMeta.rustToolchain} rustc --version >/dev/null 2>&1; then
                echo "Verus requires Rust toolchain ${verusMeta.rustToolchain}"
                echo "Install it with: rustup install ${verusMeta.rustToolchain}"
              fi
            '';
          };
        in
        {
          default = pkgs.mkShell {
            buildInputs = with pkgs; [
              rustToolchain
              pkg-config
              git
              tmux
              cargo-watch    # cargo watch -x check
              cargo-edit     # cargo add/rm
            ];

            RUST_LOG = "cwt=debug";

            shellHook = ''
              echo "cwt dev shell — rust $(rustc --version | cut -d' ' -f2)"
              echo "  cargo watch -x check    # continuous type-checking"
              echo "  cargo run -- tui         # launch TUI"
            '';
          };
        } // pkgs.lib.optionalAttrs (verusMeta != null) {
          verus = verusShell;
        }
      );

      overlays.default = final: prev: {
        cwt = self.packages.${prev.system}.default;
      };

      homeManagerModules.default = import ./nix/module.nix;
      homeManagerModules.cwt = import ./nix/module.nix;
    };
}
