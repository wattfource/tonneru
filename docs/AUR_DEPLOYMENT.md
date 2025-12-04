# AUR Deployment Guide

This document explains how to publish **tonneru** to the Arch User Repository (AUR).

## 📋 Prerequisites

### 1. AUR Account

1. Go to https://aur.archlinux.org/
2. Click "Register" and create an account
3. Verify your email

### 2. SSH Key Setup

The AUR uses SSH for git operations.

```bash
# Generate SSH key (if you don't have one)
ssh-keygen -t ed25519 -C "me@seanfournier.com"

# Start ssh-agent
eval "$(ssh-agent -s)"
ssh-add ~/.ssh/id_ed25519

# Copy your public key
cat ~/.ssh/id_ed25519.pub
```

Then:
1. Log into AUR
2. Go to "My Account"
3. Paste your public key in the "SSH Public Key" field
4. Click "Update"

### 3. Configure SSH for AUR

Add to `~/.ssh/config`:

```
Host aur.archlinux.org
    User aur
    IdentityFile ~/.ssh/id_ed25519
```

## 🚀 First-Time Publishing

### Step 1: Push Source Code to GitHub

```bash
cd /path/to/tonneru

# Add all files
git add .

# Commit
git commit -m "Initial release v0.1.0"

# Create version tag
git tag v0.1.0

# Push to GitHub (create repo first at github.com/WattForce/tonneru)
git remote add origin git@github.com:WattForce/tonneru.git
git push -u origin main --tags
```

### Step 2: Clone AUR Package Repository

```bash
# This creates an empty repo for your new package
git clone ssh://aur@aur.archlinux.org/tonneru.git ~/aur-tonneru

# If the package already exists, this clones it
# If it doesn't exist, it creates an empty repo
```

### Step 3: Copy AUR Files

```bash
cd ~/aur-tonneru

# Copy the AUR-specific files
cp /path/to/tonneru/packaging/aur/PKGBUILD .
cp /path/to/tonneru/packaging/aur/.SRCINFO .
cp /path/to/tonneru/packaging/aur/tonneru.install .
```

### Step 4: Verify PKGBUILD

```bash
# Validate the PKGBUILD
namcap PKGBUILD

# Test build locally (in a clean chroot is best)
makepkg -si

# Or test in a container
# (requires devtools package)
extra-x86_64-build
```

### Step 5: Push to AUR

```bash
cd ~/aur-tonneru

git add PKGBUILD .SRCINFO tonneru.install
git commit -m "Initial upload: tonneru 0.1.0"
git push
```

Your package is now live at: https://aur.archlinux.org/packages/tonneru

## 🔄 Updating the Package

When you release a new version:

### Step 1: Update Source and Tag

```bash
cd /path/to/tonneru

# Update version in Cargo.toml
# Update version in packaging/aur/PKGBUILD
# Update version in packaging/aur/.SRCINFO

git add .
git commit -m "Bump version to X.Y.Z"
git tag vX.Y.Z
git push origin main --tags
```

### Step 2: Update AUR

```bash
cd ~/aur-tonneru

# Copy updated files
cp /path/to/tonneru/packaging/aur/PKGBUILD .
cp /path/to/tonneru/packaging/aur/.SRCINFO .

# Commit and push
git add PKGBUILD .SRCINFO
git commit -m "Update to vX.Y.Z"
git push
```

## 📁 File Explanations

### PKGBUILD

The main build script. Key sections:

```bash
pkgname=tonneru      # Package name
pkgver=0.1.0               # Version (must match git tag without 'v')
pkgrel=1                   # Release number (increment for same pkgver)
source=(...)               # Where to download source
sha256sums=(...)           # Checksums (SKIP for git sources)

prepare() { }              # Pre-build setup (cargo fetch)
build() { }                # Compile the project
check() { }                # Run tests (optional)
package() { }              # Install files to $pkgdir
```

### .SRCINFO

Generated metadata for AUR web interface. **Must be regenerated** when PKGBUILD changes:

```bash
makepkg --printsrcinfo > .SRCINFO
```

Or manually keep in sync with PKGBUILD.

### tonneru.install

Post-install/upgrade/remove messages shown to users.

## 🔧 Troubleshooting

### "Permission denied" when pushing to AUR

```bash
# Test SSH connection
ssh -T aur@aur.archlinux.org

# Should output: "Welcome to AUR, <username>!"
# If not, check SSH key setup
```

### "Package not found" after push

- Wait a few minutes for AUR to index
- Check https://aur.archlinux.org/packages/tonneru
- Verify .SRCINFO is valid

### Build fails on AUR

```bash
# Test locally first
cd ~/aur-tonneru
makepkg -si

# Check for missing dependencies
namcap PKGBUILD
namcap tonneru-*.pkg.tar.zst
```

### Version mismatch

Ensure these all match:
- `Cargo.toml` version
- `PKGBUILD` pkgver
- `.SRCINFO` pkgver
- Git tag (vX.Y.Z)

## 📚 Resources

- [AUR Submission Guidelines](https://wiki.archlinux.org/title/AUR_submission_guidelines)
- [PKGBUILD Reference](https://wiki.archlinux.org/title/PKGBUILD)
- [Arch Packaging Standards](https://wiki.archlinux.org/title/Arch_package_guidelines)
- [Rust Packaging Guidelines](https://wiki.archlinux.org/title/Rust_package_guidelines)

## ✅ Checklist

Before publishing:

- [ ] Source code pushed to GitHub
- [ ] Git tag created (vX.Y.Z format)
- [ ] PKGBUILD version matches tag
- [ ] .SRCINFO regenerated
- [ ] Local build test passes (`makepkg -si`)
- [ ] `namcap` reports no errors
- [ ] SSH key added to AUR account

## 🆘 Need Help?

If you're helping Sean with AUR deployment:

1. **AUR Account**: Sean needs to create one at https://aur.archlinux.org
2. **SSH Key**: Help set up SSH key and add to AUR account
3. **First Push**: The first `git push` to AUR creates the package listing
4. **Maintainer**: Sean should be listed as maintainer in PKGBUILD

Contact: me@seanfournier.com | GitHub: @WattForce

