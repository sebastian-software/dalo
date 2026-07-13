# Changelog

## [0.7.0](https://github.com/sebastian-software/dalo/compare/dalo-v0.6.1...dalo-v0.7.0) (2026-07-13)


### Features

* add transactional source removal ([fa3dc6c](https://github.com/sebastian-software/dalo/commit/fa3dc6c5238951d3bf7730a21f39fd867205c801))
* improve approvals and CI checks ([6828288](https://github.com/sebastian-software/dalo/commit/6828288ccc15d406c189eaee7b497d9e527692d3))
* improve CLI status automation ([51a0012](https://github.com/sebastian-software/dalo/commit/51a0012a1e8885dd84b54432cff8af3c7a29df49))


### Bug Fixes

* address open bug reports ([2fdca9c](https://github.com/sebastian-software/dalo/commit/2fdca9c2233aae245ddd51a89bd3921bf81c45e9))
* harden site onboarding ([fe62253](https://github.com/sebastian-software/dalo/commit/fe62253fb9d255c526a90fd005a088490ad39928))
* initialize quickstart player ([8ee4fad](https://github.com/sebastian-software/dalo/commit/8ee4fad39c064d51299969e239d65923bffa7cfb))
* **inventory:** parse frontmatter with yaml_serde ([8776551](https://github.com/sebastian-software/dalo/commit/8776551110886c54fbfd09d000fc66f68b0be68d))
* publish npm package with trusted publishing ([92a53da](https://github.com/sebastian-software/dalo/commit/92a53da0f3e6b094ab0b1b9989d5c4fcd0a1fb96))
* rename npm launcher to getdalo ([77f80e1](https://github.com/sebastian-software/dalo/commit/77f80e1fab6e91d548e9e73f0e3256890c285daa))
* satisfy clippy rollback check ([14b85f1](https://github.com/sebastian-software/dalo/commit/14b85f1a3a9761155fd35fd731501362e74396dd))

## [0.6.1](https://github.com/sebastian-software/dalo/compare/dalo-v0.6.0...dalo-v0.6.1) (2026-07-13)


### Bug Fixes

* **installer:** make cosign optional ([a950484](https://github.com/sebastian-software/dalo/commit/a9504847d818a1e46d4e520fcb3de1b11666a6b7))
* **release:** use organization release token ([6e98eab](https://github.com/sebastian-software/dalo/commit/6e98eab6e9e0ebed26e03435b173934548659d31))

## [0.6.0](https://github.com/sebastian-software/dalo/compare/dalo-v0.5.0...dalo-v0.6.0) (2026-07-10)


### ⚠ BREAKING CHANGES

* require explicit approval for catalog skills

### Features

* **npm:** add the cross-platform dalo wrapper package ([500475f](https://github.com/sebastian-software/dalo/commit/500475fd29303f7970357d4d1c8680bb0fc19b53))


### Bug Fixes

* **catalog:** support newer clippy byte-string lint ([ac7848a](https://github.com/sebastian-software/dalo/commit/ac7848a18a247a5c3c7d7513652a3cd3bc1dbdd7))
* **cli:** update repair guidance for verified ownership ([bf7ba47](https://github.com/sebastian-software/dalo/commit/bf7ba47351eecc6e6756057579a1eb8c36ef798b))
* **git:** keep URL redaction lint-clean ([06c8767](https://github.com/sebastian-software/dalo/commit/06c87679eb0f1fdb4b6c9a946f7ed974a1688043))
* **git:** redact credentials from remote URLs ([779bac5](https://github.com/sebastian-software/dalo/commit/779bac5b1206270befea1b29e14d9a1a7d6c52ed))
* **installer:** create a private temporary directory ([f26f5e0](https://github.com/sebastian-software/dalo/commit/f26f5e03aa25b0de077ff711709de6d7e1576e5f))
* **lock:** fail closed on unreadable user locks ([11efa81](https://github.com/sebastian-software/dalo/commit/11efa81293fd3e14f47e71e73d6cb638ca5e7300))
* require explicit approval for catalog skills ([098805c](https://github.com/sebastian-software/dalo/commit/098805c2e7566015e1342e34a41121f958e994dd))
* **resolve:** remove only verified owned symlinks ([fc68b3a](https://github.com/sebastian-software/dalo/commit/fc68b3aed89e86c28aedf383ae1faaeb8ac21d75))
* **schema:** validate persisted schemas consistently ([43315e9](https://github.com/sebastian-software/dalo/commit/43315e9c5bf9b61d10299f478713cae0a0f69d5f))
* **security:** scope owner approvals to source provenance ([1f93cc6](https://github.com/sebastian-software/dalo/commit/1f93cc685a78371db3114bf45140edbedd38cf18))
* **state:** make instruction-pack writes recoverable ([23d92ae](https://github.com/sebastian-software/dalo/commit/23d92aec9aa2d0dcc32d7cf255ecfcdc747c3632))


### Performance Improvements

* **catalog:** hash selected catalog content lazily ([11a5c5b](https://github.com/sebastian-software/dalo/commit/11a5c5bc284497468c1cf724f0b5e1ed59b36ba1))
* **git:** remove the polling floor for short commands ([99c86bc](https://github.com/sebastian-software/dalo/commit/99c86bc25725cd2cf0e37d51f577cf2d2225d8fc))

## [0.5.0](https://github.com/sebastian-software/dalo/compare/dalo-v0.4.1...dalo-v0.5.0) (2026-07-05)


### Features

* **cli:** color terminal diagnostics ([e7c7286](https://github.com/sebastian-software/dalo/commit/e7c72866f343ca4549f98e240d9156d4d882d639))
* **cli:** improve first-run guidance ([cbecf4d](https://github.com/sebastian-software/dalo/commit/cbecf4d2d42f718e6c8a33440f8997e175301e43))


### Bug Fixes

* **cli:** hide internal git invocation details ([b74c676](https://github.com/sebastian-software/dalo/commit/b74c67648e8b625644608c2eba7b819966f78044))
* **git:** humanize network failures ([14c894e](https://github.com/sebastian-software/dalo/commit/14c894e87496fe3cf97cd48ce1d4664df1d57b02))

## [0.4.1](https://github.com/sebastian-software/dalo/compare/dalo-v0.4.0...dalo-v0.4.1) (2026-07-05)


### Bug Fixes

* **catalog:** rehash legacy source locks ([b1a5a7b](https://github.com/sebastian-software/dalo/commit/b1a5a7b9972f55822e09688e65795397e49020f4))
* **git:** harden clone and ssh handling ([cb51533](https://github.com/sebastian-software/dalo/commit/cb5153365784154af4bc60a97b1b960c077b4ba2))
* **init:** acquire store lock before repair ([194390d](https://github.com/sebastian-software/dalo/commit/194390d8d519daae2271a7778209ee8f59578f03))
* **instructions:** handle legacy relative targets ([97b9a76](https://github.com/sebastian-software/dalo/commit/97b9a760a0e7b760463b7098209b09bfb9ebed17))
* **instructions:** preserve target links and modes ([bc817b5](https://github.com/sebastian-software/dalo/commit/bc817b5d175afa7a2b292b5ef7f8c8d88f4c6e65))
* **inventory:** remove serde_yaml frontmatter parsing ([ac630f9](https://github.com/sebastian-software/dalo/commit/ac630f9b0b875fb76c92b02ef962935d14c2ec08))
* **recovery:** polish repair edge cases ([b26dd67](https://github.com/sebastian-software/dalo/commit/b26dd67c4432885434110483b88a561a34a6a974))
* **sync:** preserve links on partial source scans ([66eea6f](https://github.com/sebastian-software/dalo/commit/66eea6ff097a39d2d98b6823e9a28565450d6f99))
* **upgrade:** preserve legacy approval and slot states ([eb48ebc](https://github.com/sebastian-software/dalo/commit/eb48ebc5ef59dc1a90ee7a45913c5e348b1e2ba3))

## [0.4.0](https://github.com/sebastian-software/dalo/compare/dalo-v0.3.0...dalo-v0.4.0) (2026-07-05)


### Features

* ship completions and man page ([c06ea4f](https://github.com/sebastian-software/dalo/commit/c06ea4f8a040ea6d50676f54f618c63560978a24))


### Bug Fixes

* **adopt:** prefer slot selectors over cwd paths ([37df5a0](https://github.com/sebastian-software/dalo/commit/37df5a05bce902c84338b5ec4fe8c4cfce2e64ea))
* **adopt:** replace originals through rollback backup ([9d4d2e7](https://github.com/sebastian-software/dalo/commit/9d4d2e78db7d5d61202c0997d52341ed9ad0749e))
* **cli:** honor dry-run for mutating commands ([b1de032](https://github.com/sebastian-software/dalo/commit/b1de032430a34e879d8670433723b2d5ef1c9edf))
* **cli:** make adopt replacement explicit ([695fa42](https://github.com/sebastian-software/dalo/commit/695fa4242f390847058342be93ef96de9432a71a))
* define portable skill slot names ([94ce2f8](https://github.com/sebastian-software/dalo/commit/94ce2f8b54b1eb5069b2094f63acc35cdceb48ba))
* **git:** disable prompts and time out commands ([65f5f6e](https://github.com/sebastian-software/dalo/commit/65f5f6ee29409832f7d46a851a4fdd01f0968c55))
* harden core polish paths ([60b100c](https://github.com/sebastian-software/dalo/commit/60b100c703df726d958e5d17f56ad91b6bb2c781))
* **instructions:** reject managed marker injection ([d3c688d](https://github.com/sebastian-software/dalo/commit/d3c688d86e5a86a5290747d1b2fef83f58afab84))
* **instructions:** write target files atomically ([ee1a5cd](https://github.com/sebastian-software/dalo/commit/ee1a5cd2a2d678ce3a91eefc929489082de425b5))
* **paths:** normalize store and link targets ([b435ff4](https://github.com/sebastian-software/dalo/commit/b435ff4bbf616f08b2c2a3c6e32e99666936aa13))
* preserve instruction targets and line endings ([3085876](https://github.com/sebastian-software/dalo/commit/3085876971ce307420d09b4ed38d680719dec7cc))
* **resolve:** forget real-entry owned records ([cf6dc58](https://github.com/sebastian-software/dalo/commit/cf6dc580e849dc3192588a8e265605ea1560d302))
* **resolver:** block shadowed source requirements ([f25fad1](https://github.com/sebastian-software/dalo/commit/f25fad1ed7bcd13c56393836d17a2338bfa9d034))
* **resolver:** scope skill approvals by source ([5123825](https://github.com/sebastian-software/dalo/commit/51238257cd3e9105917ef598108a992570e305c8))
* reuse catalog snapshots during select ([0baf2d4](https://github.com/sebastian-software/dalo/commit/0baf2d48c415fa3663348549ac03e142e0d7b571))
* reuse sync resolution for lock writing ([b1dbe2b](https://github.com/sebastian-software/dalo/commit/b1dbe2b84235d13592246fcd06797cbbc321227c))
* **status:** report instruction block drift ([f429760](https://github.com/sebastian-software/dalo/commit/f429760438ee7e5b734b587b03b7d8db1931ee96))
* **status:** show all pending candidates ([798f909](https://github.com/sebastian-software/dalo/commit/798f909317ec633034c2c3479c0a500851282045))
* **store:** fsync writes and repair corrupt state ([64a5318](https://github.com/sebastian-software/dalo/commit/64a5318939cabe2cf2e95bf107437b0c19f0685a))
* **store:** use advisory file lock ([396c580](https://github.com/sebastian-software/dalo/commit/396c580020659bc46aec667a738e0b93d60bbb83))
* **sync:** block skipped materialize links ([6aad17c](https://github.com/sebastian-software/dalo/commit/6aad17cb7737fed7e4895f4feb5ed0adc963320a))
* **sync:** preserve foreign recorded symlinks ([22b166b](https://github.com/sebastian-software/dalo/commit/22b166b606d4b797574b579e28c0d1350f286f82))
* **sync:** preserve links when source scans degrade ([cbf87b4](https://github.com/sebastian-software/dalo/commit/cbf87b4a53ee49aed1affe97e2d9e05ecb837199))
* **sync:** recover unrecorded store symlinks ([9c17b00](https://github.com/sebastian-software/dalo/commit/9c17b00ad9ed6341481b2a23e7a1ef44e9dcb8b2))
* **sync:** respect source update policy ([5869009](https://github.com/sebastian-software/dalo/commit/5869009b1ad3fcb89bd15969ffbdc5e72bbbc0d4))

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
