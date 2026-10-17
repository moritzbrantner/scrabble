{
  description = "Reproducible development shell for Scrabble";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/4975466d324710c576dc11ad614684e6bd8cad8e";

  outputs =
    { nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };

          # Same Bun release and checksums as the official SHASUMS256.txt for bun-v1.4.3.
          bunAsset =
            if system == "x86_64-linux" then
              {
                archive = "bun-linux-x64";
                hash = "sha256-PnMFKNiXdfA/h/BcBnh8TfQJnQo38/JS57gpyyOBbhg=";
              }
            else
              {
                archive = "bun-linux-aarch64";
                hash = "sha256-76mBPaXtckI7+Ef5FujSxHwNd2rdlyNUAmp14Q2pqiE=";
              };

          bun = pkgs.stdenvNoCC.mkDerivation {
            pname = "bun";
            version = "1.4.3";
            src = pkgs.fetchurl {
              url = "https://github.com/oven-sh/bun/releases/download/bun-v1.4.3/${bunAsset.archive}.zip";
              inherit (bunAsset) hash;
            };
            nativeBuildInputs = [
              pkgs.autoPatchelfHook
              pkgs.unzip
            ];
            buildInputs = [ pkgs.openssl ];
            dontUnpack = true;
            installPhase = ''
              mkdir -p "$out/bin"
              unzip -q "$src"
              install -m755 "${bunAsset.archive}/bun" "$out/bin/bun"
              ln -s bun "$out/bin/bunx"
            '';
          };
        in
        {
          # rustup resolves the exact Rust toolchain from rust-toolchain.toml, which stays the
          # single Rust version authority. Playwright's Chromium and its system libraries keep
          # coming from `bunx playwright install --with-deps chromium`, as in conventional CI.
          default = pkgs.mkShell {
            packages = [
              bun
              pkgs.git
              pkgs.rustup
            ];
          };
        }
      );
    };
}
