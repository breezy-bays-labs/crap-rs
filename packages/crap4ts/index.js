// crap4ts — TypeScript adapter for the crap-rs CRAP score analyzer.
//
// Single-package distribution: this tarball ships the native cdylib for
// every platform the publish workflow builds (linux x64-gnu, darwin
// arm64, darwin x64). Selecting the right one at require-time keeps
// the npm install lean enough that we don't yet need the scoped
// platform-subpackage / optionalDependencies layout used by larger
// napi-rs crates.

'use strict';

const { existsSync } = require('node:fs');
const { join } = require('node:path');

const { platform, arch } = process;

function nativeBindingFilename() {
  switch (platform) {
    case 'darwin':
      switch (arch) {
        case 'arm64':
          return 'crap4ts.darwin-arm64.node';
        case 'x64':
          return 'crap4ts.darwin-x64.node';
        default:
          throw new Error(
            `crap4ts: unsupported macOS architecture: ${arch}. ` +
              `Published architectures for 2.0.0-rc.1: arm64, x64.`,
          );
      }
    case 'linux':
      if (arch === 'x64') {
        return 'crap4ts.linux-x64-gnu.node';
      }
      throw new Error(
        `crap4ts: unsupported Linux architecture: ${arch}. ` +
          `Published architectures for 2.0.0-rc.1: x64 (glibc).`,
      );
    default:
      throw new Error(
        `crap4ts: unsupported platform: ${platform}. ` +
          `Published platforms for 2.0.0-rc.1: darwin, linux.`,
      );
  }
}

const filename = nativeBindingFilename();
const localPath = join(__dirname, filename);

if (!existsSync(localPath)) {
  throw new Error(
    `crap4ts: native binding ${filename} not found in ${__dirname}. ` +
      `The npm package may be incomplete or installed for the wrong platform.`,
  );
}

const nativeBinding = require(localPath);

if (!nativeBinding || typeof nativeBinding.analyze !== 'function') {
  throw new Error(
    `crap4ts: native binding ${filename} loaded but is missing the ` +
      `'analyze' export. The cdylib may be from an incompatible build.`,
  );
}

module.exports = { analyze: nativeBinding.analyze };
