let
  pkgs = import <nixpkgs> { };
  fx2 = pkgs.python3Packages.buildPythonPackage rec {
    pname = "fx2";
    version = "0.8";
    # not using Pypi as it lacks the firmware sources
    src = pkgs.fetchFromGitHub {
      owner = "whitequark";
      repo = "libfx2";
      rev = "v${version}";
      sha256 = "0b3zp50mschsxi2v3192dmnpw32gwblyl8aswlz9a0vx1qg3ibzn";
    };
    nativeBuildInputs = with pkgs; [ gnumake sdcc ];
    propagatedBuildInputs = with pkgs.python3Packages; [ libusb1 crcmod ];
    preBuild = ''
      cd software
      python setup.py build_ext
    '';
  };
  nmigen = pkgs.python3Packages.buildPythonPackage rec {
    name = "nmigen";
    version = "0.1rc1";
    src = pkgs.fetchgit {
      url = "https://github.com/m-labs/nmigen";
      rev = "v${version}";
      sha256 = "002kyxazpc2azarfzahr17imizs0m9m5silggzx8yyy1kljxl796";
      leaveDotGit = true;
    };
    checkPhase = "PATH=${pkgs.yosys}/bin:${pkgs.symbiyosys}/bin:${pkgs.yices}/bin:$PATH python -m unittest discover nmigen.test -v";
    nativeBuildInputs = with pkgs; [ python3Packages.setuptools_scm git ];
    propagatedBuildInputs = with pkgs.python3Packages; [ bitarray pyvcd jinja2 ];
  };
  glasgow = pkgs.python3Packages.buildPythonApplication rec {
    pname = "glasgow";
    version = "2019-10-07";
    src = pkgs.fetchgit {
      url = "https://github.com/GlasgowEmbedded/Glasgow";
      rev = "bfe49bebc4483b32eed8ec127a98a9fa2e77e7d4";
      sha256 = "0sh7l57pr49mmiqxq6g12pcln9h474gvlkfh3c439aqiv3p3jvyl";
      fetchSubmodules = true;
      leaveDotGit = true;
    };
    patches = [ ./glasgow-applet.diff ];
    nativeBuildInputs = with pkgs; [ python3Packages.setuptools_scm git gnumake sdcc ];
    propagatedBuildInputs = (
      [ fx2 nmigen ] ++
      (with pkgs.python3Packages; [ setuptools libusb1 aiohttp pyvcd bitarray crcmod ]) ++
      (with pkgs; [ yosys nextpnr icestorm ]));
    preBuild = ''
      cd software
      python setup.py build_ext
    '';
    # tests are currently broken since nMigen migration
    doCheck = false;
  };
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
  }
