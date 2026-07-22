# Changelog

## [0.10.1](https://github.com/sebastian-software/dalo/compare/dalo-v0.10.0...dalo-v0.10.1) (2026-07-22)


### Bug Fixes

* close instruction target race windows ([a245f53](https://github.com/sebastian-software/dalo/commit/a245f537dd1617195cb5ec619d61d8dcf3ad4b2e))
* contain inventory symlinks and target rollback ([3b5fb93](https://github.com/sebastian-software/dalo/commit/3b5fb9313b8aec61ab502d77f8f71889183db713))
* harden adoption selectors and comparisons ([3d8958f](https://github.com/sebastian-software/dalo/commit/3d8958f4161a544e8a1364a8052fcb409c3fc181))
* make instruction target writes conditional ([3e93c0d](https://github.com/sebastian-software/dalo/commit/3e93c0deb7de5a2e776a8ebd7e6644058bf78ebf))
* protect instruction pack mutations ([42ee685](https://github.com/sebastian-software/dalo/commit/42ee685854b6c5b676fdc71d30386a0be9de138b))

## [0.10.0](https://github.com/sebastian-software/dalo/compare/dalo-v0.9.2...dalo-v0.10.0) (2026-07-22)


### Features

* **agent:** add portable agent package foundation ([4b8973b](https://github.com/sebastian-software/dalo/commit/4b8973be2d5d1f6fe64136119286bf5379f11f0b))
* **audit:** rename reviewer selector ([1707f8f](https://github.com/sebastian-software/dalo/commit/1707f8fc9c7775d8d51cf2d964a69d6ea07e3f05))


### Bug Fixes

* harden portable agent previews ([cad182d](https://github.com/sebastian-software/dalo/commit/cad182deedcfd67b7b5c251b6790c2c4e2920379))
* harden team catalog updates ([5c641a5](https://github.com/sebastian-software/dalo/commit/5c641a5dd9de491b2871138d8275f5ac7a9c141a))
* keep catalog refresh consistent ([0255dc9](https://github.com/sebastian-software/dalo/commit/0255dc92b0142bc37c0221e7cc84b1a53b4cf1bc))
* satisfy strict clippy byte lint ([b8876e0](https://github.com/sebastian-software/dalo/commit/b8876e01d7f2ce978d1c844ba0de755aadb7f9e3))
* serialize catalog checks with advances ([b004792](https://github.com/sebastian-software/dalo/commit/b004792c52568d9d48477dbfd3dbaf819fdd6988))

## [0.9.2](https://github.com/sebastian-software/dalo/compare/dalo-v0.9.1...dalo-v0.9.2) (2026-07-20)


### Bug Fixes

* **approve:** allow revoking approvals for unresolvable sources/skills ([8ef27d1](https://github.com/sebastian-software/dalo/commit/8ef27d165aae849bf06374543aaacf521e433091))
* **approve:** point non-catalog sources at status for a missing skill ([8233153](https://github.com/sebastian-software/dalo/commit/8233153f20e175e7b4db81030a92e8d2d4e3d77a))
* **audit:** skip special files in the agent snapshot to avoid a hang ([69e45bf](https://github.com/sebastian-software/dalo/commit/69e45bf6bd9bcf4324247fae9f9a60d218a834c2))
* **autosync:** bound the append-only scheduler logs ([eaffa98](https://github.com/sebastian-software/dalo/commit/eaffa987b173c6e2151af4e3934fe21b274b4c59))
* **autosync:** force C locale for scheduler commands; root-safe test ([4e0663a](https://github.com/sebastian-software/dalo/commit/4e0663aa9e4f54c696c83c734fe0a280f15802b0))
* **autosync:** keep the tail bytes when a log line has no newline ([06065fa](https://github.com/sebastian-software/dalo/commit/06065fae4282eacee63b0ce02e4434cbfd40130d))
* **autosync:** validate scheduler artifact count to prevent a panic ([d412c21](https://github.com/sebastian-software/dalo/commit/d412c21f9077ff031d3afdc0a988d9b3cb78f9b6))
* **catalog:** clean up SHA-256 (64-char) staging worktrees ([36bfc3e](https://github.com/sebastian-software/dalo/commit/36bfc3e9a4ca08eec9166038c1836783aa13cac4))
* **cli:** report audit/state blocks without the "check failed:" prefix ([5ea5bbc](https://github.com/sebastian-software/dalo/commit/5ea5bbc85c663532032405875793db0699af4fc4))
* **doctor:** distinguish an unreadable source checkout from a missing one ([732e394](https://github.com/sebastian-software/dalo/commit/732e3940602a1bcbffcd404eda5d5f0efa700bca))
* **doctor:** report missing sources and use runnable next_command hints ([43b91c5](https://github.com/sebastian-software/dalo/commit/43b91c54cec509c8d680516b6ec77b4206434cf2))
* **doctor:** run resolution checks even when the lock is corrupt ([db18b2f](https://github.com/sebastian-software/dalo/commit/db18b2f9725213c73249782d5f870f9a0753db95))
* **doctor:** run the security-audit gate so doctor matches status/sync ([d370453](https://github.com/sebastian-software/dalo/commit/d37045375b1c9d50c0dd94eaadbb6faef26f025c))
* **git:** ignore untracked files when checking source dirtiness ([142a285](https://github.com/sebastian-software/dalo/commit/142a285d4b45e46787750692a83c95bc773e05a7))
* **git:** reject Git revision expressions as manifest pins ([de51028](https://github.com/sebastian-software/dalo/commit/de51028c1a7385e17a5110604b00f97d31322932))
* **resolver:** clarify the blocked-winner alternate hint wording ([90efbc4](https://github.com/sebastian-software/dalo/commit/90efbc4925a5bbea196bc710e2e1ab8636426aaf))
* **resolver:** disclose an equal-priority tie in the shadow message ([5a1ee36](https://github.com/sebastian-software/dalo/commit/5a1ee36763f26d1e8b15d24106b49035ac1487cc))
* **resolver:** surface an approved alternate when a slot's winner is blocked ([3e0a88f](https://github.com/sebastian-software/dalo/commit/3e0a88f7b197b75fcc749240e4d7a16d3ecd4a7c))
* **team:** validate catalog version during manifest validation ([3a242b3](https://github.com/sebastian-software/dalo/commit/3a242b33f5abf6b6e6fa973deb48101af47b4483))

## [0.9.1](https://github.com/sebastian-software/dalo/compare/dalo-v0.9.0...dalo-v0.9.1) (2026-07-18)


### Bug Fixes

* **autosync:** explain disabled state, flag interrupted runs, bound logs ([98c9f90](https://github.com/sebastian-software/dalo/commit/98c9f908d745f49b065853ae608467178d2247b1)), closes [#364](https://github.com/sebastian-software/dalo/issues/364)
* **autosync:** gate the stale-run status hint on installed ([8a9dc7f](https://github.com/sebastian-software/dalo/commit/8a9dc7fc793cfba33617ca203d8eac97829c83e5))
* clarify legacy namespace migration ([eec4202](https://github.com/sebastian-software/dalo/commit/eec42021380c79fb75452cd13d2a570edf2c4d94))
* disambiguate team catalog source ids ([e398cce](https://github.com/sebastian-software/dalo/commit/e398cce199254cad79d9ca5da0461c963ae6c50b))
* **doctor:** stop dead-end hints for missing store files; align docs ([9b416dc](https://github.com/sebastian-software/dalo/commit/9b416dc1f9cbface496cb67794b6b316c198e7a3)), closes [#366](https://github.com/sebastian-software/dalo/issues/366)
* emit valid systemd autosync log targets ([97d3a94](https://github.com/sebastian-software/dalo/commit/97d3a9403a62cdeb4e901cd60f0b343bd9eba13d))
* **error:** stop labeling plain state errors as "check failed" ([a05d616](https://github.com/sebastian-software/dalo/commit/a05d6168f1480808bb1819b6234af72540383932)), closes [#362](https://github.com/sebastian-software/dalo/issues/362)
* keep autosync executable paths stable ([5d8b8c2](https://github.com/sebastian-software/dalo/commit/5d8b8c227711ba8eff5325a14619515cf68db07c))
* narrow autosync path guards ([a4010cf](https://github.com/sebastian-software/dalo/commit/a4010cf2527e3fcfe077c5cd8c07f6b5e159a562))
* preserve systemd autosync log paths ([bd22b24](https://github.com/sebastian-software/dalo/commit/bd22b2467ba90e49c0cb1a00e1a3c53a1fbd65ba))
* preserve unreadable autosync metadata ([1cd383a](https://github.com/sebastian-software/dalo/commit/1cd383ac8ed3e3a37b6981f7c0f9d4c178e1cbf6))
* recover corrupt autosync install state ([4418a4b](https://github.com/sebastian-software/dalo/commit/4418a4b4bd9cad4e3578470a9434641278655fba))
* **site:** surface autosync on the homepage; align quickstart video ([b411fcf](https://github.com/sebastian-software/dalo/commit/b411fcff8bdd731b178137e800c2ebbc8c290fd3)), closes [#367](https://github.com/sebastian-software/dalo/issues/367)
* **team:** cap manifest reads, label staging debris, document inert --store ([17bf5f0](https://github.com/sebastian-software/dalo/commit/17bf5f0778bb620506479519c6aee618517d3b75)), closes [#365](https://github.com/sebastian-software/dalo/issues/365)

## [0.9.0](https://github.com/sebastian-software/dalo/compare/dalo-v0.8.2...dalo-v0.9.0) (2026-07-17)


### Features

* add installable scheduled autosync ([34603de](https://github.com/sebastian-software/dalo/commit/34603de7603fcbc5f86e84f79ef1a67a1b9c5c32))
* add reviewed catalog pin advancement ([04e01ed](https://github.com/sebastian-software/dalo/commit/04e01ed717a1cc79cec40a98514b885cb1165c54))
* add team manifest management CLI ([24fd25d](https://github.com/sebastian-software/dalo/commit/24fd25db5c539f7216471be8b946a334ec1f6e2a))
* compose team catalog manifests ([7116618](https://github.com/sebastian-software/dalo/commit/71166185f8afdb74860b2e9cbd72f71b3780fd64))
* replace quickstart asciicast with MP4 video ([fcc1440](https://github.com/sebastian-software/dalo/commit/fcc1440c3a571fe7bda6357ecf7236fcfb407f19))


### Bug Fixes

* distinguish invalid CLI arguments ([b503c6e](https://github.com/sebastian-software/dalo/commit/b503c6e21152366125f02f5f3843ee3cb1c831d1))
* polish CLI guidance and output ([b5e1d13](https://github.com/sebastian-software/dalo/commit/b5e1d13e56a5b78ffb6bdb923732c8be8e8d53bf))
* preserve team manifest diagnostics ([78dbe61](https://github.com/sebastian-software/dalo/commit/78dbe61175943ffefe7bdb529813280d04cf8900))
* report catalog selection mutations ([78aed48](https://github.com/sebastian-software/dalo/commit/78aed480c3a3af47a77f921412195f565b9f99fd))
* report exact status check reasons ([4f592b5](https://github.com/sebastian-software/dalo/commit/4f592b51f4ad417cd4ff5d299ebb9d78013fbf30))

## [0.8.2](https://github.com/sebastian-software/dalo/compare/dalo-v0.8.1...dalo-v0.8.2) (2026-07-15)


### Bug Fixes

* **audit:** block persistence and privileged execution ([954e3b3](https://github.com/sebastian-software/dalo/commit/954e3b3f0f61a5b7ab477e0eb2cfca7c41696ce0))
* **audit:** cover privilege escalation variants ([7fc21d5](https://github.com/sebastian-software/dalo/commit/7fc21d5c42aa643fab40f049cad78caa21259132))
* **audit:** make agent reviews non-authoritative ([e3436e3](https://github.com/sebastian-software/dalo/commit/e3436e36236dbdab7255dc8ff5a92a559e3fa841))
* **release:** harden update distribution edges ([1f1ef60](https://github.com/sebastian-software/dalo/commit/1f1ef609bfa279affb48ce6530a2916889d8a9fe))
* **status:** surface sync blockers ([f560564](https://github.com/sebastian-software/dalo/commit/f560564aca2345ccc9dccfe01bfbee9f462059e6))

## [0.8.1](https://github.com/sebastian-software/dalo/compare/dalo-v0.8.0...dalo-v0.8.1) (2026-07-14)


### Bug Fixes

* isolate catalog and target recovery ([a6abf9f](https://github.com/sebastian-software/dalo/commit/a6abf9f8a7935519e7989795740441c3ac8a9d43))

## [0.8.0](https://github.com/sebastian-software/dalo/compare/dalo-v0.7.2...dalo-v0.8.0) (2026-07-14)


### Features

* add Homebrew tap release integration ([54d7149](https://github.com/sebastian-software/dalo/commit/54d7149ee6254f4ffd6e14325329422887cfde8f))
* add platform-aware install picker ([66cdaa8](https://github.com/sebastian-software/dalo/commit/66cdaa81c5c549acb232082fc27e80f81b2099ed))


### Bug Fixes

* close catalog drift integrity gaps ([7be72d1](https://github.com/sebastian-software/dalo/commit/7be72d134af5083240d647da8cbb13dec52d311b))
* document degraded source recovery ([eb630c6](https://github.com/sebastian-software/dalo/commit/eb630c6d905406ef05eca4051d867eed0d1f090f))
* enforce required closure at link time ([24fbfd2](https://github.com/sebastian-software/dalo/commit/24fbfd27bc8f30ec02686277de72a5f4fb61351d))
* harden install picker fallbacks ([96f7e8e](https://github.com/sebastian-software/dalo/commit/96f7e8eab76e7e5bc41504aa2655869729f9f399))
* improve CLI help accuracy ([c01fcb1](https://github.com/sebastian-software/dalo/commit/c01fcb1c720a540a85230ca17cc279373e181a6c))
* make npm launcher offline resilient ([5799fd1](https://github.com/sebastian-software/dalo/commit/5799fd147b7571f2d742972ecedc72ae19f724e0))
* make state and keep protection resilient ([61d0fc8](https://github.com/sebastian-software/dalo/commit/61d0fc84856323ae2c47aae3c63c476931b24aa9))
* migrate every catalog lock inventory ([fbad9e3](https://github.com/sebastian-software/dalo/commit/fbad9e35268e0eeda08dc4089abd8b26cd48ea58))
* order cached prereleases by semver ([78b8716](https://github.com/sebastian-software/dalo/commit/78b8716e092c57b70f1185a91a43598fa1d016cf))
* preserve additive state fields on rewrite ([6386824](https://github.com/sebastian-software/dalo/commit/6386824bde36c4de3a38befd82b19f7a9df038e2))
* preserve moved target metadata ([3c74374](https://github.com/sebastian-software/dalo/commit/3c743744d13118b10b2a0f21dba3d1d058407c3b))
* preserve no-script picker fallback ([b571091](https://github.com/sebastian-software/dalo/commit/b571091963bf6678370781ef9eb5337d71d6e22e))
* reject lossy state metadata merges ([e572d1b](https://github.com/sebastian-software/dalo/commit/e572d1b10009fdd299e5e984e86ce9c2630e33cd))
* scope link conflicts per target ([e440b09](https://github.com/sebastian-software/dalo/commit/e440b09d676b97112a47d72053143b8d23feead0))
* support legacy release tags in tap dispatch ([a81444f](https://github.com/sebastian-software/dalo/commit/a81444f41d53e1ddbc3e3f39f9f3eaee34ba3ace))

## [0.7.2](https://github.com/sebastian-software/dalo/compare/dalo-v0.7.1...dalo-v0.7.2) (2026-07-14)


### Bug Fixes

* **deps:** update googleapis/release-please-action action to v5 ([bebd02f](https://github.com/sebastian-software/dalo/commit/bebd02ffd59499c3b00294ded737e99d27303a34))
* harden inventory scanning ([befecb0](https://github.com/sebastian-software/dalo/commit/befecb04dd9554649a6c0048494cd6375cb661ca))
* harden source lifecycle recovery ([2ac9b1f](https://github.com/sebastian-software/dalo/commit/2ac9b1f2b6ff047953f6e45870a4241a7fec7002))
* make recovery hints actionable ([7eb5bea](https://github.com/sebastian-software/dalo/commit/7eb5beaeb58b27e2efb5b92024e1925e08251f53))
* prefer existing local source paths ([9227751](https://github.com/sebastian-software/dalo/commit/9227751335fe0f6667347a183737051d00ab23c0))
* preserve valid inventory paths ([a3c558a](https://github.com/sebastian-software/dalo/commit/a3c558a44ee02a81f47edc61d6fa9cd50130b44d))
* quote recovery paths safely ([4e9ce40](https://github.com/sebastian-software/dalo/commit/4e9ce40dfaba08343000274322d8aadc963c1abe))
* surface actionable sync diagnostics ([45f270e](https://github.com/sebastian-software/dalo/commit/45f270e5d9851802c87bfb80d880b82b74684bc1))

## [0.7.1](https://github.com/sebastian-software/dalo/compare/dalo-v0.7.0...dalo-v0.7.1) (2026-07-13)


### Bug Fixes

* **deps:** update rust crate clap_mangen to 0.3 ([40fdb3d](https://github.com/sebastian-software/dalo/commit/40fdb3db2704a764dcf1a73b75642bc1a2b14432))
* **deps:** update rust crate sha2 to 0.11 ([906da18](https://github.com/sebastian-software/dalo/commit/906da188c81d07b2a4b6229ee09fd65cacd1d460))
* **deps:** update sigstore/cosign-installer action to v4.1.2 ([09fa5ad](https://github.com/sebastian-software/dalo/commit/09fa5ad5bdb8ff0c567d5c57281061e4cd990f4b))
* harden sync recovery paths ([c0bad4a](https://github.com/sebastian-software/dalo/commit/c0bad4a8034b8569fe1c0c095b5922f4498b6eef))
* preserve recovery state on sync failure ([30aa383](https://github.com/sebastian-software/dalo/commit/30aa383edd2b25b7226c9b7fd5531fb7255b8b3d))

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
