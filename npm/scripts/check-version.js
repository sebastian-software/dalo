'use strict';

const fs = require('node:fs');
const path = require('node:path');

const cargoManifest = process.env.DALO_CARGO_TOML
  ?? path.resolve(__dirname, '../../Cargo.toml');
const packageManifest = process.env.DALO_PACKAGE_JSON
  ?? path.resolve(__dirname, '../package.json');
const cargo = fs.readFileSync(cargoManifest, 'utf8');
const packageVersion = JSON.parse(fs.readFileSync(packageManifest, 'utf8')).version;
const cargoVersion = cargo.match(/^version\s*=\s*"([^"]+)"$/m)?.[1];

if (!cargoVersion) throw new Error(`could not read package version from ${cargoManifest}`);
if (packageVersion !== cargoVersion) {
  throw new Error(`npm package version ${packageVersion} must match Cargo version ${cargoVersion}`);
}
