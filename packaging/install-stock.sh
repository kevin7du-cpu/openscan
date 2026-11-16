#!/bin/sh
set -eu
source_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
name=$(basename -- "$source_dir")
case "$name" in OpenScan-0.3.1-Soft3D-Light-r4-linux-x64) ;; *) echo 'Run from the extracted OpenScan 0.3.1 package.' >&2; exit 1;; esac
(cd "$source_dir" && sha256sum --check --status SHA256SUMS.txt)
root="$HOME/.local/opt/openscan"
destination="$root/versions/$name"
if [ -e "$destination" ]; then echo "Already installed: $destination" >&2; exit 1; fi
for link in "$root/current" "$HOME/.local/bin/openscan"; do
  if [ -e "$link" ] && [ ! -L "$link" ]; then echo "Refusing to replace a regular file: $link" >&2; exit 1; fi
done
mkdir -p "$root/versions" "$HOME/.local/bin" "$HOME/.local/share/applications"
cp -R -- "$source_dir" "$destination"
chmod 755 "$destination/openscan"
(cd "$destination" && sha256sum --check --status SHA256SUMS.txt)
cat > "$destination/launch.sh" <<'LAUNCH'
#!/bin/sh
set -eu
root=$(dirname -- "$(readlink -f -- "$0")")
export OPENSCAN_MODELS="$root/models"
exec "$root/openscan" "$@"
LAUNCH
chmod 755 "$destination/launch.sh"
ln -s -- "$destination" "$root/current.new-$$"
mv -Tf -- "$root/current.new-$$" "$root/current"
ln -sfn -- "$root/current/launch.sh" "$HOME/.local/bin/openscan"
if [ -f "$HOME/.local/share/applications/openscan.desktop" ]; then
  cp -p "$HOME/.local/share/applications/openscan.desktop" "$destination/previous-launcher.desktop"
fi
cat > "$HOME/.local/share/applications/openscan.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=OpenScan 0.3.1 Soft 3D Light
Exec="$HOME/.local/bin/openscan"
Icon=scanner
Terminal=false
Categories=Office;Graphics;
DESKTOP
echo "Installed $name. Previous versions and user data retained."
