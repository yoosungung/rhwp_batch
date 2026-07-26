# rhwp-batch

> Linux 환경에서 동작하는 CLI 도구. 데이터 처리 파이프라인(Airflow / K8s `Job` 등)에서
> HWP/HWPX 문서를 다룬다.
>
> 1. **변환**: HWP/HWPX → JSON (텍스트·표·이미지 포함)
> 2. **생성**: 양식(template) HWP + JSON → 채워진 HWP (배치 모드 1급 시민)

> ⚠️ **v0.8.2** — upstream [`rhwp` v0.8.2](https://github.com/edwardkim/rhwp/releases/tag/v0.8.2) 추적.
> Linux·macOS 바이너리는 [GitHub Releases](https://github.com/yoosungung/rhwp_batch/releases)에서 받을 수 있다.
> 진척 상황은 [ROADMAP.md](ROADMAP.md), 기술 설계는 [DESIGN.md](DESIGN.md),
> 기여자 작업 규칙은 [CLAUDE.md](CLAUDE.md) 참조.

---

## 무엇을 하는가

```
       Airflow DAG / K8s Job
             │ exec
             ▼
       ┌──────────────────────┐
       │   rhwp-batch CLI     │
       └──────────────────────┘
        │            │
   /templates/  /input/  →  /output/
   (양식 HWP)  (HWP 또는 JSON)   (변환·생성 결과)
```

- 입출력은 **마운트된 디렉토리**(PVC, ConfigMap, S3 sync sidecar 등)로 주고받는다.
- 양식 HWP는 **컨테이너 이미지에 굽지 않는다** — 외부 경로에서 로드한다.
- 양식 변경 시 이미지 재빌드 없이 ConfigMap/오브젝트 스토리지 갱신만으로 즉시 반영된다.

---

## 설치

### 바이너리 (GitHub Releases)

[`v0.8.2` 릴리즈](https://github.com/yoosungung/rhwp_batch/releases/tag/v0.8.2)에서 플랫폼에 맞는 tarball을 받는다.

| 파일 | 대상 |
|------|------|
| `rhwp-batch-v0.8.2-x86_64-unknown-linux-gnu.tar.gz` | Linux x86_64 (glibc) |
| `rhwp-batch-v0.8.2-aarch64-apple-darwin.tar.gz` | macOS Apple Silicon |

```bash
# 예: Linux
curl -LO https://github.com/yoosungung/rhwp_batch/releases/download/v0.8.2/rhwp-batch-v0.8.2-x86_64-unknown-linux-gnu.tar.gz
curl -LO https://github.com/yoosungung/rhwp_batch/releases/download/v0.8.2/rhwp-batch-v0.8.2-x86_64-unknown-linux-gnu.tar.gz.sha256
shasum -a 256 -c rhwp-batch-v0.8.2-x86_64-unknown-linux-gnu.tar.gz.sha256
tar -xzf rhwp-batch-v0.8.2-x86_64-unknown-linux-gnu.tar.gz
./rhwp-batch --version
```

### 사전 요구사항 (소스 빌드)

- [Rust](https://rustup.rs/) 1.70 이상 (`rustc`, `cargo`)
- Git

```bash
# Rust 미설치 시 (Linux·macOS 공통)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustc --version
```

### Linux (소스 빌드)

```bash
git clone https://github.com/yoosungung/rhwp_batch.git
cd rhwp_batch

# 릴리즈 빌드 (rhwp 의존성 포함, 수 분 소요)
cargo build --release -p rhwp-batch

# PATH에 설치 (둘 중 하나)
cargo install --path rhwp-batch --force          # ~/.cargo/bin/
sudo install -m 755 target/release/rhwp-batch /usr/local/bin/

rhwp-batch --version
```

운영·파이프라인 환경에서는 아래 **컨테이너 이미지** 사용을 권장한다.

### macOS (소스 빌드, 로컬 개발·검증용)

공식 운영 타깃은 Linux이지만, 로컬에서 변환·양식 채우기를 시험할 때 동일한 절차로 빌드할 수 있다.

```bash
# Xcode Command Line Tools (최초 1회)
xcode-select --install

git clone https://github.com/yoosungung/rhwp_batch.git
cd rhwp_batch

cargo build --release -p rhwp-batch
cargo install --path rhwp-batch --force          # ~/.cargo/bin/ (PATH에 없으면 ~/.zshrc 등에 추가)

rhwp-batch --version
```

### 컨테이너 이미지 (운영 권장)

```bash
docker pull registry.local/rhwp-batch:0.8.2
```

직접 이미지를 빌드할 때 (먼저 Linux 릴리즈 바이너리를 `dist/`에 둔다):

```bash
cargo build --release -p rhwp-batch
install -Dm755 target/release/rhwp-batch dist/rhwp-batch
docker build -f deploy/Dockerfile.batch -t rhwp-batch:local .
docker run --rm rhwp-batch:local --version
```

> 사내 레지스트리 주소·태그는 운영 정책에 맞춰 사용한다.

### 개발자 참고

테스트·워크스페이스 구조는 [CLAUDE.md "빌드 및 실행"](CLAUDE.md) 참조.

```bash
cargo test -p rhwp-batch              # 단위·통합 테스트
cargo test -p rhwp-batch <test_name>  # 단일 테스트
```

---

## CLI 사용법

### 공통 옵션

```text
--log-format {text|json}     # JSON 로그는 잡 러너에서 파싱하기 쉬움
--log-level info|debug|trace
--version
--help
```

### `to-json` — HWP/HWPX → JSON

```bash
# 단일 파일
rhwp-batch to-json INPUT.hwp [-o OUTPUT.json]

# 디렉토리 일괄
rhwp-batch to-json --input-dir IN/ --output-dir OUT/

# 옵션
  --image-mode {extract|inline}   # 기본 extract (별도 파일). inline은 base64 (메모리 비용 큼)
  --image-dir DIR                 # extract 모드일 때 이미지 출력 위치 (기본: <input>.assets/)
  --pretty                        # JSON 들여쓰기
  --schema-version 1.0.0          # 출력 스키마 버전 명시
  --heading-style-map "제목1=1,개요1=1,제목2=2"  # 양식별 heading 스타일 매핑 보정
```

JSON 스키마는 **RAG ingestion 입력**을 목적으로 설계된 DTO다. 평탄한 `blocks: []`
배열에 의미 단위(heading/paragraph/table/image/header/footer/footnote/caption)를
담아 청크화·임베딩 파이프라인이 그대로 받아쓸 수 있게 한다. IR 디테일
(`line_segs`, `char_shapes` 등)은 노출하지 않는다. 정본 스키마는
[ROADMAP.md §1.3](ROADMAP.md), 매핑 설계는 [rhwp-batch/DESIGN.md §3.2](rhwp-batch/DESIGN.md) 참조.

### `fill` — 양식 + JSON → HWP

```bash
# 단일 채우기
rhwp-batch fill \
  --template /templates/order.hwp \
  --data /input/order_001.json \
  -o /output/order_001.hwp

# 배치 모드 (양식 1번만 파싱)
rhwp-batch fill \
  --template /templates/order.hwp \
  --data-dir /input/orders/ \
  --output-dir /output/ \
  --output-pattern '{stem}.hwp'

# 매니페스트 파일 (JSONL: {"data":..., "out":"..."})
rhwp-batch fill \
  --template /templates/order.hwp \
  --manifest /input/jobs.jsonl

# 옵션
  --on-missing-key {error|empty|keep}  # 마커 미해결 정책
  --on-error {fail-fast|continue}      # 배치 내 일부 실패 처리
  --report /output/_report.json        # 배치 결과 리포트
  --threads N                          # 병렬 처리
```

#### 배치 모드 (1 양식 + N JSON)

양식은 1회만 파싱하고 IR을 N건 작업에 재사용한다 → 콜드 스타트 비용을 분산.

```text
parse_hwp(template) → Document (1회)
for each json in inputs:
    let doc = template.clone()      # 메모리 복제만
    apply_template_data(doc, json)  # 마커 치환·표·이미지
    serialize_hwp(doc) → 출력
```

기술적 근거는 [DESIGN.md "배치 모드의 IR 재사용"](DESIGN.md) 참조.

### 종료 코드

| 코드 | 의미 |
|------|------|
| 0    | 모든 작업 성공 |
| 1    | 인자 오류 |
| 2    | 입력 파일 오류 (없음/형식 불일치) |
| 3    | 양식 처리 오류 |
| 4    | 부분 실패 (배치에서 일부 실패, `--on-error continue` 시) |
| 10+  | 내부 오류 |

---

## 양식 작성 가이드

양식 HWP에 다음 형태의 **마커**를 한컴오피스로 작성해 둔다.

| 마커 | 용도 | 예시 |
|------|------|------|
| `{{name}}` | 단순 텍스트 치환 | `{{order_id}}`, `{{customer.name}}` |
| `{{table:rows.items[].col}}` | 표 행 동적 확장 | `{{table:rows.items[].sku}}` |
| `{{image:key}}` | 이미지 삽입 | `{{image:logo}}` |

자세한 문법, 양식 작성 시 주의사항, CharShape(글꼴/크기/색상) 보존 규칙은
별도 양식 매뉴얼 문서(예정)에서 다룬다.

---

## 사용 예시

### Kubernetes Job

```yaml
apiVersion: batch/v1
kind: Job
metadata:
  generateName: rhwp-fill-
spec:
  template:
    spec:
      restartPolicy: OnFailure
      containers:
      - name: rhwp
        image: registry.local/rhwp-batch:0.1.0
        args:
          - fill
          - --template=/templates/order.hwp
          - --data-dir=/input
          - --output-dir=/output
          - --report=/output/_report.json
          - --log-format=json
        volumeMounts:
          - { name: templates, mountPath: /templates, readOnly: true }
          - { name: input,     mountPath: /input,     readOnly: true }
          - { name: output,    mountPath: /output }
      volumes:
        - name: templates
          configMap: { name: rhwp-templates }
        - name: input
          persistentVolumeClaim: { claimName: rhwp-input-pvc }
        - name: output
          persistentVolumeClaim: { claimName: rhwp-output-pvc }
```

### Airflow (KubernetesPodOperator)

운영 가이드 및 DAG 예시는 `deploy/k8s/airflow-dag-example.py` 및 별도 운영 가이드
문서(예정) 참조.

---

## 비-목표 (사용자가 알아야 할 범위 한계)

본 도구는 다음을 **지원하지 않는다**.

- HTTP / gRPC 서버 (잡 러너에서 직접 실행하는 모델)
- 인증/권한 (잡 단위 격리에 의존)
- HWP ↔ PDF 직접 변환 (필요 시 별도 도구 사용)
- 양식 스토리지 자체 구현 (S3/GCS sync는 K8s sidecar 책임)
- 한컴 자체 양식 형식 `.hwt` 직접 지원
- 실시간 협업 편집, 사용자 관리, 멀티 테넌시
- HWPX **출력**(v1 제외, v2 도입 예정 — 입력 파싱은 v1부터 지원)

---

## 추가 문서

| 문서 | 대상 | 내용 |
|------|------|------|
| [DESIGN.md](DESIGN.md) | 개발자·리뷰어 | rhwp 자산 매핑, IR 모델, DocumentCore 활용, 라운드트립 보장 |
| [ROADMAP.md](ROADMAP.md) | 계획자·기여자 | 결정 항목 D1~D22, 마일스톤 M1~M6, 위험 관리 |
| [CLAUDE.md](CLAUDE.md) | 클로드 코드·기여자 | 작업 규칙(하이퍼-워터폴), 워크스페이스, 빌드 |

---

## 참고 및 감사 (Acknowledgements)

본 프로젝트는 **`rhwp`** crate (HWP/HWPX 파서·`DocumentCore` 편집 엔진·시리얼라이저)를
**무수정 재사용**하는 별도 CLI 도구다. `rhwp` 가 없었다면 본 프로젝트는 성립하지 않는다.

- **원본 저장소 (upstream)**: <https://github.com/edwardkim/rhwp>
- **작성자**: Edward Kim
- **라이선스**: MIT (Copyright © 2025-2026 Edward Kim)
- 본 저장소는 `rhwp` crate를 워크스페이스 멤버로 복사해 사용하며, **무수정 원칙**을 유지한다
  (자세한 정책은 [CLAUDE.md](CLAUDE.md), 자산 매핑은 [DESIGN.md](DESIGN.md) 참조).

`rhwp` 의 설계 의도(WASM/PyO3/MCP 등 어떤 어댑터에서도 `DocumentCore`를 독립 사용 가능)
덕분에, 본 CLI 도구는 별도 포크나 패치 없이 깔끔하게 어댑터 계층만 얹어 구현할 수 있었다.
이 자리에 `rhwp` 작성자 **Edward Kim** 님께 감사를 표한다.

---

## 라이선스

본 저장소(`rhwp-batch`) 자체의 라이선스 정책은 상위 `rhwp` crate(MIT)와 호환되도록
정한다. 자세한 내용은 `LICENSE`(예정) 참조.
