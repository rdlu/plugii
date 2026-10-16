# AUR packages

Two packages, each its own AUR git repository:

| Directory | AUR package | Builds from |
| --- | --- | --- |
| `plugii/` | [`plugii`](https://aur.archlinux.org/packages/plugii) | the GitHub tag tarball, `cargo build --frozen --release`, with the test suite in `check()` |
| `plugii-bin/` | [`plugii-bin`](https://aur.archlinux.org/packages/plugii-bin) | the release workflow's tarballs, x86_64 and aarch64 |

Both install the binary, the man pages, bash/zsh/fish completions, README,
CHANGELOG and the MIT license (`/usr/share/licenses/<pkgname>/`).
Dependencies: `glibc`, `libgcc` and `systemd-libs` (libudev) at runtime;
`plugii` also needs `cargo` to build (`systemd-libs` carries libudev's
headers and `libudev.pc`, and `pkgconf` is in base-devel). `fwupd` is
optional, for `--firmware`.

The `sha256sums` are `SKIP` placeholders until the tag and release exist:
fill them in with `updpkgsums` (step 2 below) before the first push.

`plugii` sets `!lto`: makepkg's LTO flags reach the C code that ring
(rustls) compiles, and the link then fails with undefined `ring_core_*`
symbols.

## Testing locally

```sh
mise run aur-test
```

builds the source PKGBUILD with `makepkg` from a `git archive` of HEAD in a
temporary directory (prepare, build, the tests in check, package), then runs
`namcap` on the PKGBUILD and the package, in an `archlinux` container with
podman when namcap isn't installed. `mise run release-check` also checks
that each `.SRCINFO` is current and that `pkgver` matches Cargo.toml.

## Updating for a release

After the release workflow has published `vX.Y.Z` on GitHub:

1. In both `PKGBUILD`s set `pkgver=X.Y.Z` and `pkgrel=1`. (`pkgrel` goes
   up instead when only the packaging changes.)
2. Fill in the checksums, which downloads the tag tarball and the release
   tarballs:

   ```sh
   (cd packaging/aur/plugii && updpkgsums)
   (cd packaging/aur/plugii-bin && updpkgsums)
   ```

   `updpkgsums` is in `pacman-contrib`.
3. Regenerate the `.SRCINFO` files: `mise run aur-srcinfo`.
4. Check, then commit to this repository:

   ```sh
   mise run aur-test        # the source package, against HEAD
   (cd packaging/aur/plugii-bin && makepkg -f && namcap PKGBUILD *.pkg.tar.zst)
   git commit -am "Update the AUR packages to X.Y.Z"
   ```

## Publishing (the maintainer's step)

Needs an AUR account with your SSH key. The AUR repositories hold only
`PKGBUILD` and `.SRCINFO`, so they are separate clones, not this directory:

```sh
# Once per package; on the first push this creates the package.
git clone ssh://aur@aur.archlinux.org/plugii.git ~/aur/plugii
git clone ssh://aur@aur.archlinux.org/plugii-bin.git ~/aur/plugii-bin

# Each release:
cp packaging/aur/plugii/{PKGBUILD,.SRCINFO} ~/aur/plugii/
cp packaging/aur/plugii-bin/{PKGBUILD,.SRCINFO} ~/aur/plugii-bin/
cd ~/aur/plugii && git add PKGBUILD .SRCINFO && git commit -m "Update to X.Y.Z" && git push
cd ~/aur/plugii-bin && git add PKGBUILD .SRCINFO && git commit -m "Update to X.Y.Z" && git push
```

The AUR only accepts the `master` branch. If your AUR SSH key is the
security key, a push can fail until you touch it: retry, as with GitHub.
