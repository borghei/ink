{
  lib,
  stdenv,
  rustPlatform,
  installShellFiles,
  versionCheckHook,
}:

let
  cargoToml = lib.importTOML ../Cargo.toml;
in
rustPlatform.buildRustPackage {
  # Not `ink`: nixpkgs already has an unrelated package by that name.
  pname = cargoToml.package.name;
  inherit (cargoToml.package) version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../LICENSE
      ../src
      ../tests
      ../examples
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [ installShellFiles ];

  # Tests run in the flake's checks; enable them in nixpkgs.
  doCheck = false;

  postInstall = ''
    install -Dm644 LICENSE -t $out/share/doc/$pname
  ''
  + lib.optionalString (stdenv.buildPlatform.canExecute stdenv.hostPlatform) ''
    $out/bin/ink man > ink.1
    installManPage ink.1
    installShellCompletion --cmd ink \
      --bash <($out/bin/ink completions bash) \
      --zsh <($out/bin/ink completions zsh) \
      --fish <($out/bin/ink completions fish)
  '';

  nativeInstallCheckInputs = [ versionCheckHook ];
  versionCheckProgramArg = "--version";
  doInstallCheck = true;

  meta = {
    description = "Terminal markdown reader with support for images, diagrams and math";
    homepage = cargoToml.package.repository;
    changelog = "${cargoToml.package.repository}/blob/v${cargoToml.package.version}/CHANGELOG.md";
    # Redistribution is allowed but selling is not, which nixpkgs counts as unfree.
    license = {
      fullName = "ink License";
      url = "${cargoToml.package.repository}/blob/main/LICENSE";
      free = false;
      redistributable = true;
    };
    mainProgram = "ink";
    platforms = lib.platforms.unix;
  };
}
