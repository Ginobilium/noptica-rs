# This is for nixpkgs 19.09

let
  pkgs = import <nixpkgs> { };
  glasgow = pkgs.callPackage ./glasgow.nix {};
  pyqtgraph-qt5 = pkgs.python3Packages.buildPythonPackage rec {
    name = "pyqtgraph_qt5-${version}";
    version = "0.10.0";
    doCheck = false;
    src = pkgs.fetchFromGitHub {
      owner = "pyqtgraph";
      repo = "pyqtgraph";
      rev = "1426e334e1d20542400d77c72c132b04c6d17ddb";
      sha256 = "1079haxyr316jf0wpirxdj0ry6j8mr16cqr0dyyrd5cnxwl7zssh";
    };
    propagatedBuildInputs = with pkgs.python3Packages; [ scipy numpy pyqt5 pyopengl ];
  };
in
 pkgs.mkShell {
    buildInputs = [
      glasgow
      (pkgs.python3.withPackages(ps: [ps.quamash ps.pyqt5 pyqtgraph-qt5]))
      pkgs.rustc pkgs.cargo
    ];

    # Hack: shut up rustc complaint "#![feature] may not be used on the stable release channel"
    RUSTC_BOOTSTRAP = "1";
  }
