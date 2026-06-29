# Changelog

## [0.3.0](https://github.com/sebastian-software/dalo/compare/dalo-v0.2.0...dalo-v0.3.0) (2026-06-29)


### Features

* add catalog sources with inspect and select (M11) ([aca74db](https://github.com/sebastian-software/dalo/commit/aca74dbb4471a4b0a305a74e1bd8b4b227dc22cc))
* detect catalog drift with a read-only refresh check (M12) ([3a8a576](https://github.com/sebastian-software/dalo/commit/3a8a576cfdccb93eb6bce5230e6efb91527becaa))
* discover instruction packs and warn on topic overlap (M15) ([a0e6460](https://github.com/sebastian-software/dalo/commit/a0e64603d6b221b1dcde881d0196c4b10edb01c6))
* expand and preflight same-catalog required closures (M13) ([4269132](https://github.com/sebastian-software/dalo/commit/426913261da5926e0ff6e69d343e9275507e2ee0))
* render instruction packs into managed blocks (M14) ([b758ef2](https://github.com/sebastian-software/dalo/commit/b758ef26942f57d0504ec1bedad0b097c02cd97d))


### Bug Fixes

* harden V1.1 catalog and instruction edge cases ([fcfdbcb](https://github.com/sebastian-software/dalo/commit/fcfdbcb1b22b36097155587cdb810d3f759b1cdf))

## [0.2.0](https://github.com/sebastian-software/dalo/compare/dalo-v0.1.0...dalo-v0.2.0) (2026-06-25)


### Features

* add doctor diagnostics ([d4e3718](https://github.com/sebastian-software/dalo/commit/d4e37187196a0d8bba1b904b7778d251412fdf18))
* add skill inventory scanner ([4864311](https://github.com/sebastian-software/dalo/commit/4864311f44c64b45d128fb602a062478f5b76305))
* add target registry commands ([b53ff1a](https://github.com/sebastian-software/dalo/commit/b53ff1a294675e2829cd11c34df2ef835233e47f))
* add team source git safety ([d538592](https://github.com/sebastian-software/dalo/commit/d538592c63d19436843f44994273b6990b768b1b))
* adopt unmanaged skills ([c0aa4b1](https://github.com/sebastian-software/dalo/commit/c0aa4b1e5ec936941ffabd4f244f94aea2f1d5da))
* implement resolver status ([824a86a](https://github.com/sebastian-software/dalo/commit/824a86ad50bcf62af8d3a5a3aba981228975f871))
* implement store init ([c494169](https://github.com/sebastian-software/dalo/commit/c4941698339d0fa044ac85b8741653583d9f1909))
* implement trusted-source approval ([#20](https://github.com/sebastian-software/dalo/issues/20)) ([3eef780](https://github.com/sebastian-software/dalo/commit/3eef7806a1317dddd8262f54f96774a05ed43974))
* materialize local sync ([b919a8a](https://github.com/sebastian-software/dalo/commit/b919a8adc08098f9316d6a1441da8e35c2883a6d))
* persist resolved user lock ([34c715b](https://github.com/sebastian-software/dalo/commit/34c715b46ff5dbf0aa7935572868c8536bc38057))
* scaffold rust cli project ([6be3512](https://github.com/sebastian-software/dalo/commit/6be351218930d27ad9b91bb5f1c3501864460ee6))


### Bug Fixes

* guard adopt --yes against an unrelated pre-existing local skill ([01dc4ab](https://github.com/sebastian-software/dalo/commit/01dc4abd825e8e3d677fbe3641ab13c510126320))
* harden filesystem and git-clone safety ([#1](https://github.com/sebastian-software/dalo/issues/1)) ([be7d3ea](https://github.com/sebastian-software/dalo/commit/be7d3ea2d34b737e86006b7b3f28851f4e5a96ac))
* harden parsing, platform declaration, and error context ([#3](https://github.com/sebastian-software/dalo/issues/3)) ([73ded07](https://github.com/sebastian-software/dalo/commit/73ded07e9bcb5e44b4309bf1a09d039c0fa3ad32))
* make global CLI flags honest ([#2](https://github.com/sebastian-software/dalo/issues/2)) ([0c4af26](https://github.com/sebastian-software/dalo/commit/0c4af2659d7cbf74387bf1cc31ccb86bdc36b418))
* protect local override and fix shared-target id mapping ([#18](https://github.com/sebastian-software/dalo/issues/18)) ([433d1b9](https://github.com/sebastian-software/dalo/commit/433d1b94f7b911aff6e4899a333f62fcabaabc75))
* repair the two-step adopt flow ([#30](https://github.com/sebastian-software/dalo/issues/30)) ([df77575](https://github.com/sebastian-software/dalo/commit/df7757560b8170a204cf662b729f137137f0ea2a))
* self-heal materialization_dirs for pre-existing stores ([#29](https://github.com/sebastian-software/dalo/issues/29)) ([3023373](https://github.com/sebastian-software/dalo/commit/30233730ef92956cb336e642b118e55c30abf2de))
* store lock, schema validation, adopt path, source cleanup ([#19](https://github.com/sebastian-software/dalo/issues/19)) ([bf0be84](https://github.com/sebastian-software/dalo/commit/bf0be844ccbf8540e40d11764a44397a7df9a25d))
* validate source ids and folder slot names ([#17](https://github.com/sebastian-software/dalo/issues/17)) ([bf9131f](https://github.com/sebastian-software/dalo/commit/bf9131f1782a501e712d4fff34dc875d3835011b))
