#!/usr/bin/env bash
# rhwp-batch/tests/scripts/convert_all.sh
#
# 4개 소스 디렉토리(`rhwp/output`, `rhwp/samples`, `rhwp/saved`,
# `rhwp-batch/samples`)에 들어 있는 모든 HWP/HWPX 파일을 release 빌드된
# `rhwp-batch to-json`으로 변환하여 `rhwp-batch/tests/output/<label>/`에
# 저장한다. 출력 폴더는 `.gitignore`로 관리된다.
#
# 사용:
#   bash rhwp-batch/tests/scripts/convert_all.sh           # 전체
#   bash rhwp-batch/tests/scripts/convert_all.sh --debug   # 디버그 빌드 사용
#
# 종료 코드:
#   0 — 모든 소스 디렉토리 처리 완료 (개별 파일 실패가 있어도 0;
#        --on-error처럼 부분 실패 분기는 호출자 책임)
#   2 — release 바이너리 부재
#
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
PROFILE="release"
if [ "${1:-}" = "--debug" ]; then
  PROFILE="debug"
fi
BIN="$ROOT/target/$PROFILE/rhwp-batch"
OUT_BASE="$ROOT/rhwp-batch/tests/output"

if [ ! -x "$BIN" ]; then
  echo "ERROR: binary not found at $BIN" >&2
  echo "  build first:  cargo build --release -p rhwp-batch" >&2
  exit 2
fi

mkdir -p "$OUT_BASE"

run_one() {
  local label="$1"
  local src="$2"
  if [ ! -d "$src" ]; then
    printf '  skip %-20s (missing: %s)\n' "$label" "$src"
    return
  fi

  local in_count
  in_count=$(find -L "$src" -type f \( -iname '*.hwp' -o -iname '*.hwpx' \) | wc -l | tr -d ' ')
  if [ "$in_count" = "0" ]; then
    printf '  skip %-20s (no HWP/HWPX files)\n' "$label"
    return
  fi

  local dest="$OUT_BASE/$label"
  mkdir -p "$dest"
  printf '==> %-20s %4s files: %s -> %s\n' "$label" "$in_count" "$src" "$dest"

  # to-json은 디렉토리 모드에서 개별 파일 실패를 카운트하지만 종료 코드는
  # 비-제로일 수 있으므로 호출자에서 ||true 처리.
  "$BIN" to-json \
    --input-dir "$src" \
    --output-dir "$dest" \
    --image-mode extract \
    --pretty \
    --overwrite \
    --log-format text \
    --log-level warn \
    || printf '   (some files in %s failed — see warnings above)\n' "$label"

  local out_count
  out_count=$(find "$dest" -type f -name '*.json' | wc -l | tr -d ' ')
  printf '    -> %s json files written\n' "$out_count"
}

echo "rhwp-batch convert-all"
echo "  binary: $BIN"
echo "  output: $OUT_BASE"
echo

run_one rhwp_output         "$ROOT/rhwp/output"
run_one rhwp_samples        "$ROOT/rhwp/samples"
run_one rhwp_saved          "$ROOT/rhwp/saved"
run_one rhwp_batch_samples  "$ROOT/rhwp-batch/samples"

echo
echo "done."
