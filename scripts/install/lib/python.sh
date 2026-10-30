# python.sh — pinned uv and Python (step_python).
# Fragment of the generated install.sh: assembled by scripts/install/build-install.sh,
# never sourced at runtime. Edit this file, then regenerate install.sh.

# ---------------------------------------------------------------------------
# Pinned uv and Python (spec step 2)
# ---------------------------------------------------------------------------

sha256_file() {
    _f="$1"
    _line=""
    if command -v sha256sum >/dev/null 2>&1; then
        _line=$(sha256sum "$_f") || return 1
    elif command -v shasum >/dev/null 2>&1; then
        _line=$(shasum -a 256 "$_f") || return 1
    else
        return 2
    fi
    printf '%s\n' "${_line%% *}"
}

uv_target() {
    case "$OS:$ARCH" in
        macos:arm64) printf '%s\n' "aarch64-apple-darwin" ;;
        macos:x86_64) printf '%s\n' "x86_64-apple-darwin" ;;
        linux:x86_64) printf '%s\n' "x86_64-unknown-linux-gnu" ;;
        linux:arm64) printf '%s\n' "aarch64-unknown-linux-gnu" ;;
        *) die "unsupported platform $OS/$ARCH for the pinned uv $UV_VERSION archive" ;;
    esac
}

# URL of the archive to fetch. ORBIT_OFFICE_UV_URL is a seam for the
# harness; it does not select the expected digest.
uv_archive_url() {
    if [ -n "${ORBIT_OFFICE_UV_URL:-}" ]; then
        printf '%s\n' "$ORBIT_OFFICE_UV_URL"
        return 0
    fi
    printf 'https://github.com/astral-sh/uv/releases/download/%s/uv-%s.tar.gz\n' \
        "$UV_VERSION" "$(uv_target)"
}

# Digest recorded for this archive name. Empty when the name is not pinned.
uv_expected_sha256() {
    _base="$1"
    _base=${_base%%\?*}
    _base=${_base%%#*}
    _base=${_base%/}
    _base=${_base##*/}
    case "$_base" in
        uv-aarch64-apple-darwin.tar.gz) printf '%s\n' "$UV_SHA256_MACOS_ARM64" ;;
        uv-x86_64-apple-darwin.tar.gz) printf '%s\n' "$UV_SHA256_MACOS_X86_64" ;;
        uv-x86_64-unknown-linux-gnu.tar.gz) printf '%s\n' "$UV_SHA256_LINUX_X86_64" ;;
        uv-aarch64-unknown-linux-gnu.tar.gz) printf '%s\n' "$UV_SHA256_LINUX_AARCH64" ;;
        uv-test-fixture.tar.gz) printf '%s\n' "$UV_SHA256_FIXTURE" ;;
        *) printf '\n' ;;
    esac
}

uv_download_cleanup() {
    rm -rf "$TOOLS_DIR/.download"
    rmdir "$TOOLS_DIR" 2>/dev/null || true
}

# Install tools/bin/uv from the pinned archive. Never uses a uv already
# on PATH. Checksum failure returns before tar runs.
install_pinned_uv() {
    command -v curl >/dev/null 2>&1 || die "curl is required to download uv $UV_VERSION"
    _url=$(uv_archive_url)
    _expected=$(uv_expected_sha256 "$_url")
    if [ -z "$_expected" ]; then
        die "uv: no recorded SHA-256 for $_url; refusing to unpack"
    fi
    mkdir -p "$TOOLS_DIR"
    _work="$TOOLS_DIR/.download"
    rm -rf "$_work"
    mkdir -p "$_work"
    _archive="$_work/uv.tar.gz"
    if ! curl -fsSL "$_url" -o "$_archive"; then
        uv_download_cleanup
        die "uv: failed to download $_url"
    fi
    _sha_rc=0
    _got=$(sha256_file "$_archive") || _sha_rc=$?
    if [ "$_sha_rc" -eq 2 ]; then
        uv_download_cleanup
        die "sha256sum or shasum is required to verify the uv archive"
    fi
    if [ "$_sha_rc" -ne 0 ] || [ -z "${_got:-}" ]; then
        uv_download_cleanup
        die "uv: could not compute the SHA-256 of the downloaded archive"
    fi
    if [ "$_got" != "$_expected" ]; then
        uv_download_cleanup
        die "uv archive SHA-256 mismatch for $_url: expected $_expected, got $_got; not unpacked"
    fi
    command -v tar >/dev/null 2>&1 || {
        uv_download_cleanup
        die "tar is required to unpack the uv archive"
    }
    mkdir -p "$_work/unpack"
    if ! tar -xzf "$_archive" -C "$_work/unpack"; then
        uv_download_cleanup
        die "uv: failed to unpack the archive"
    fi
    _bin=""
    for _cand in "$_work/unpack"/*/uv "$_work/unpack"/uv; do
        if [ -f "$_cand" ] && [ ! -L "$_cand" ]; then
            _bin="$_cand"
            break
        fi
    done
    if [ -z "$_bin" ]; then
        uv_download_cleanup
        die "uv: archive contains no uv binary"
    fi
    mkdir -p "$TOOLS_BIN"
    cp "$_bin" "$TOOLS_BIN/uv.tmp.$$"
    chmod 755 "$TOOLS_BIN/uv.tmp.$$"
    mv "$TOOLS_BIN/uv.tmp.$$" "$TOOLS_BIN/uv"
    rm -rf "$_work"
}

uv_is_pinned() {
    [ -x "$TOOLS_BIN/uv" ] || return 1
    _uv_out=$("$TOOLS_BIN/uv" --version 2>/dev/null || true)
    _uv_got=$(printf '%s\n' "$_uv_out" | awk 'NR == 1 { print $2 }')
    [ "$_uv_got" = "$UV_VERSION" ]
}

# True when python3 --version prints the pinned minor (3.12 or 3.12.x).
python_version_ok() {
    [ -x "$TOOLS_BIN/python3" ] || return 1
    _py_out=$("$TOOLS_BIN/python3" --version 2>&1) || return 1
    _py_got=$(printf '%s\n' "$_py_out" | awk 'NR == 1 { print $2 }')
    case "$_py_got" in
        "$PYTHON_VERSION"|"$PYTHON_VERSION".*) return 0 ;;
        *) return 1 ;;
    esac
}

python_already_installed() {
    uv_is_pinned || return 1
    [ -e "$TOOLS_BIN/python" ] || return 1
    [ -e "$TOOLS_BIN/python3" ] || return 1
    python_version_ok
}

install_pinned_python() {
    mkdir -p "$TOOLS_PYTHON" "$TOOLS_CACHE" "$TOOLS_BIN"
    # --no-bin (also UV_PYTHON_INSTALL_BIN=0) keeps uv from linking
    # interpreters into ~/.local/bin. The cache stays under tools/.
    # Confirmed on the pinned uv: `python install --help` lists both.
    if ! UV_PYTHON_INSTALL_DIR="$TOOLS_PYTHON" \
        UV_PYTHON_INSTALL_BIN=0 \
        UV_CACHE_DIR="$TOOLS_CACHE" \
        "$TOOLS_BIN/uv" python install --no-bin --cache-dir "$TOOLS_CACHE" "$PYTHON_VERSION"
    then
        die "uv python install $PYTHON_VERSION failed"
    fi
    _py=$(UV_PYTHON_INSTALL_DIR="$TOOLS_PYTHON" \
        UV_CACHE_DIR="$TOOLS_CACHE" \
        "$TOOLS_BIN/uv" python find --no-project "$PYTHON_VERSION") || \
        die "uv python find $PYTHON_VERSION failed"
    case "$_py" in
        "$TOOLS_PYTHON"/*) ;;
        *) die "uv python find $PYTHON_VERSION returned '${_py:-nothing}', outside $TOOLS_PYTHON" ;;
    esac
    [ -x "$_py" ] || die "uv python find $PYTHON_VERSION is not executable: $_py"
    ln -sfn "$_py" "$TOOLS_BIN/python3"
    ln -sfn "$_py" "$TOOLS_BIN/python"
    if ! python_version_ok; then
        _py_out=$("$TOOLS_BIN/python3" --version 2>&1 || true)
        die "python: $TOOLS_BIN/python3 --version must print the pinned minor version $PYTHON_VERSION; got: ${_py_out:-nothing}"
    fi
    say "python: installed uv $UV_VERSION and Python $PYTHON_VERSION ($TOOLS_DIR)"
}

step_python() {
    step "2 python"
    if python_already_installed; then
        say "python: already done (uv $UV_VERSION and Python $PYTHON_VERSION in $TOOLS_DIR)"
        return 0
    fi
    # A uv on PATH is ignored; only the binary we just verified is used.
    if ! uv_is_pinned; then
        install_pinned_uv
    fi
    install_pinned_python
}

