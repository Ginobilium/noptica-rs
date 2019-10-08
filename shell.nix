let
  yosys_overlay = self: super:
    rec {
      yosys = super.yosys.overrideAttrs(oa: rec {
        name = "yosys-${version}";
        version = "0.9";
        srcs = [
          (super.fetchFromGitHub {
            owner  = "yosyshq";
            repo   = "yosys";
            rev    = "yosys-${version}";
            sha256 = "0lb9r055h8y1vj2z8gm4ip0v06j5mk7f9zx9gi67kkqb7g4rhjli";
            name   = "yosys";
          })
          # NOTE: the version of abc used here is synchronized with
          # the one in the yosys Makefile of the version above;
          # keep them the same for quality purposes.
          (super.fetchFromGitHub {
            owner  = "berkeley-abc";
            repo   = "abc";
            rev    = "3709744c60696c5e3f4cc123939921ce8107fe04";
            sha256 = "18a9cjng3qfalq8m9az5ck1y5h4l2pf9ycrvkzs9hn82b1j7vrax";
            name   = "yosys-abc";
          })
        ];
        buildInputs = oa.buildInputs ++ [ super.zlib ];
      });
    };
  pkgs = import <nixpkgs> { overlays = [ yosys_overlay ]; };
  fx2 = pkgs.python3Packages.buildPythonPackage rec {
    pname = "fx2";
    version = "0.7";
    # not using Pypi as it lacks the firmware sources
    src = pkgs.fetchFromGitHub {
      owner = "whitequark";
      repo = "libfx2";
      rev = "v${version}";
      sha256 = "0xvlmx6ym0ylrvnlqzf18d475wa0mfci7wkdbv30gl3hgdhsppjz";
    };
    nativeBuildInputs = with pkgs; [ gnumake sdcc ];
    propagatedBuildInputs = with pkgs.python3Packages; [ libusb1 crcmod ];
    preBuild = ''
      cd software
      python setup.py build_ext
    '';
  };
  nmigen = pkgs.python3Packages.buildPythonPackage {
    name = "nmigen";
    version = "2019-08-26";
    src = pkgs.fetchFromGitHub {
      owner = "m-labs";
      repo = "nmigen";
      rev = "2168ff512bfe04806b35c09d3b1d265a16c4ddbc";
      sha256 = "0ij9idvlqsjzzr50vyg2ziabj7lv7yi8s0826g3acrn45hfv4535";
    };
    checkPhase = "PATH=${pkgs.yosys}/bin:${pkgs.symbiyosys}/bin:${pkgs.yices}/bin:$PATH python -m unittest discover nmigen.test -v";
    propagatedBuildInputs = with pkgs.python3Packages; [ bitarray pyvcd jinja2 ];
  };
  glasgow = pkgs.python3Packages.buildPythonApplication rec {
    pname = "glasgow";
    version = "2019-08-28";
    src = pkgs.fetchFromGitHub {
      owner = "GlasgowEmbedded";
      repo = "Glasgow";
      rev = "c103a8fc7945a0e46fb8b50fab63c51efe27e242";
      sha256 = "0mfzjf74w71yasbj9jvdx86ipc3wrmxiqa819b586k9dsskzgw32";
      fetchSubmodules = true;
    };
    patches = [ ./glasgow-applet.diff ];
    nativeBuildInputs = with pkgs; [ gnumake sdcc ];
    propagatedBuildInputs = (
      [ fx2 nmigen ] ++
      (with pkgs.python3Packages; [ libusb1 aiohttp pyvcd bitarray crcmod ]) ++
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
