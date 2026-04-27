# ROADMAP.md — rhwp-batch 구축 계획 (v1.0)

> **목표**: Linux 환경에서 동작하는 **CLI 도구**를 신설하여
> 사용자의 데이터 처리 파이프라인(Airflow / K8s Job 등)에서
> 다음을 수행한다.
>
> 1. **변환**: HWP/HWPX 문서 → JSON (표·이미지 포함)
> 2. **생성**: JSON + 양식(template) HWP/HWPX → 채워진 HWP/HWPX
>    - 양식은 **외부 경로에서 로드** (컨테이너에 굽지 않음)
>    - **배치 모드**: 양식 1개 + JSON N개 → HWP N개 (양식 1번만 파싱)
>
> **다른 문서**:
> - 사용자 가이드(완성 후 사양): [README.md](README.md)
> - 기술 분석: [DESIGN.md](DESIGN.md)
> - 작업 계약·주의사항: [CLAUDE.md](CLAUDE.md)
>
> | 일자 | 버전 | 변경 |
> |------|------|------|
> | 2026-04-27 | 0.1 | 최초 작성 (HTTP 서버 가정) |
> | 2026-04-27 | 0.2 | CLI/배치 모델로 전환, 양식 외부 경로화 |
> | 2026-04-27 | 0.3 | 4문서 역할 분리 (README/CLAUDE/DESIGN/ROADMAP), 워크스페이스 구조 확정 |
> | 2026-04-27 | 0.4 | M0 제거, D-NEW1/2 → D21/22 재번호, 신규 결정 D12~D20 추가, server_* 명명 정리 |
> | 2026-04-27 | 0.5 | **DTO를 RAG-friendly 스키마로 확정** (D2). D4/D13 확정 (extract 기본·SemVer). 신규 D23~D29 확정. M1 단계 4단계로 확장. |
> | 2026-04-27 | 0.6 | D6~D11 §1.1로 일괄 이동 확정. D8 = v1 NanumGothic 단독 확정. git 원격 셋업(github.com/yoosungung/rhwp_batch) 완료. M1 착수 준비 완료. |
| 2026-04-27 | 1.0 | **M1~M5 구현 완료**. 14종 테스트 통과. M6 v1 제외(D5 확정). |

---

## 0. 큰 그림

```
┌─────────────────────────────────────────────────────────────────┐
│   Airflow DAG / K8s Job                                         │
│       │                                                         │
│       ▼  exec                                                   │
│   ┌─────────────────────────────────────────────────────────┐   │
│   │  rhwp-batch (CLI 바이너리, 신규)                       │   │
│   │                                                         │   │
│   │   ┌──────────┐    ┌─────────────┐    ┌──────────────┐ │   │
│   │   │  CLI     │───▶│  Service    │───▶│  Adapter     │ │   │
│   │   │  (clap)  │    │  변환/생성   │    │  IR ↔ DTO    │ │   │
│   │   └──────────┘    │  배치 러너   │    └──────────────┘ │   │
│   │                   └─────────────┘                       │   │
│   └─────────────────────────────────────────────────────────┘   │
│                            │ use                                │
│                            ▼                                    │
│   ┌─────────────────────────────────────────────────────────┐   │
│   │  rhwp (기존 crate, 무수정)                              │   │
│   │  parser · DocumentCore · serializer                     │   │
│   └─────────────────────────────────────────────────────────┘   │
│                                                                 │
│  Volume mounts:                                                 │
│   /templates  ← 양식 HWP (외부 경로, ConfigMap/PVC/S3 sync)     │
│   /input      ← 처리 대상 (HWP 또는 JSON 데이터)                │
│   /output     ← 결과 파일                                       │
└─────────────────────────────────────────────────────────────────┘
```

핵심 원칙:

- 워크스페이스 멤버 `rhwp` 크레이트는 **무수정 원칙** (필요 시 상위 저장소에 별도 PR).
- 신규 코드는 모두 워크스페이스 멤버 `rhwp-batch`에 배치.
- 잡 러너(Airflow/K8s) 위에 **HTTP 서버를 추가로 두지 않는다**.
- 양식 HWP는 **컨테이너 이미지에 포함하지 않고** 외부 경로에서 로드.
- 콜드 스타트 비용은 **배치 모드(1 양식 + N JSON)**로 분산.

---

## 1. 결정 사항

### 1.1 확정 (사용자 결정)

| # | 항목 | 결정 |
|---|------|------|
| **D1** | 인터페이스 | **CLI 도구** (HTTP 서버 X) |
| **D2** | `to-json` JSON 스키마 | **RAG-friendly DTO** (§1.3 정본). 평탄 `blocks: []`, `heading_path` 누적, 표는 markdown+rows 이중 표현, 이미지는 asset 참조 + 인접 텍스트. |
| **D3** | 양식 채우기 마커 외형 | `{{name}}`, `{{table:JMESPath}}`, `{{image:key}}` 3개 prefix만 v1. `date:`/`if:` 등은 v2 검토. |
| **D4** | 이미지 운반 방식 | **기본 `extract`** (별도 파일 + path 참조). `--image-mode inline` 명시 시만 base64. RAG 메모리 비용 회피. |
| **D5** | HWPX 출력 | **v1 제외, v2 도입** (`serialize_hwpx` 미완성) |
| **D6** | 양식 외부 경로 출처 | **로컬 파일 시스템(마운트) v1**. S3·GCS는 v2 (sidecar로 sync). |
| **D7** | 인증/권한 | **미포함**. 잡 단위 격리(K8s namespace/serviceaccount)에 의존. |
| **D8** | 폰트 동봉 | **v1 = NanumGothic만** (`fonts-nanum`/`fonts-nanum-coding`). 사내 표준 폰트는 v2 별도 이미지 빌드. |
| **D9** | 이미지 배포 | **Docker multi-stage** + 사내 레지스트리 (`registry.local/rhwp-batch:0.1.0`). |
| **D10** | 실패 처리 | 배치 내 단일 실패 → 해당 항목만 실패 + **exit code 4** (부분 실패). Airflow 재시도는 잡 단위. |
| **D11** | 양식 캐시 TTL | **프로세스 수명** 동안 유지 (배치 끝나면 종료). 별도 캐시 레이어 없음. |
| **D12** | 마커 키 표현식 문법 | **JMESPath 표준** 채택 (`rows.items[].col` 등 자체 정의 금지) |
| **D13** | `schema_version` 운용 정책 | **SemVer 엄격**. 초기값 `1.0.0`. 메이저 불일치 시 consumer 거부 의무, 마이너는 forward-compatible (선택 필드 추가 가능). `to-json`은 항상 `schema_version` 출력. |
| **D14** | 누락 키 정책 기본값 | `--on-missing-key` 기본 **`error`** (안전 기본값, 양식 검증 강제) |
| **D15** | `--threads` 기본값 | **CPU 수** (`std::thread::available_parallelism`). 미지정 시 자동. |
| **D16** | 마커 escape 문법 | **v1 미지원**. 양식에 `{{` 리터럴 금지(가이드에 명시). v2에서 도입 검토. |
| **D17** | 출력 파일 충돌 정책 | 기본 **거부 + 종료 코드 3**. `--overwrite` 플래그로 명시 허용. |
| **D18** | 페이지네이션 비용 | v1은 `*_native()` 호출당 페이지네이션 트리거를 **감수**. 측정 후 임계 초과 시 v1.1에서 rhwp 트랜잭션 PR. |
| **D19** | 샘플 데이터 출처 | `rhwp/samples/`의 일부를 `rhwp-batch/samples/from_rhwp/`에 심볼릭 링크. 양식 신규 작성분은 `samples/templates/`. |
| **D20** | 측정 도구 | `tracing`의 span timing(`#[tracing::instrument]` + JSON 로그)으로 충분. 별도 벤치 프레임워크 도입 보류. |
| **D21** *(구 D-NEW1)* | 배치 모드 | **양식 1 + JSON N → HWP N** 1회 호출 지원 (1급 시민) |
| **D22** *(구 D-NEW2)* | 양식 위치 | **외부 경로** (`--template-path`, 컨테이너 미포함) |
| **D23** | heading 스타일 매핑 | 휴리스틱(스타일명 "제목N" → level N) + `--heading-style-map` 사용자 override |
| **D24** | 머리말/꼬리말 처리 | 항상 `type: header`/`footer` 블록으로 출력. 필터링은 downstream 책임. |
| **D25** | 표 병합셀 처리 | 첫 셀에만 값 보존, 나머지 빈 문자열, `merged_cells: [{r,c,rowspan,colspan}]` 별도 |
| **D26** | 표 셀 내부 다단 문단 | Markdown 출력 시 `<br>` 결합 (raw 보존 안 함). v1 단순화. |
| **D27** | 출력 모드 | v1은 **단일 문서 JSON만**. JSONL/blocks 청크 출력은 v2. 단, 스키마는 (b)로 자연스럽게 변환되도록 설계됨. |
| **D28** | 텍스트 정규화 강도 | **NFC만**. 공백 압축·특수문자 변환은 downstream chunker 책임. |
| **D29** | 표·그림 caption | 별도 `type: caption` 블록 + `ref_block_id`로 표/그림 블록과 연결 |

> §1.2 (작업 시작 전 합의 필요) 표는 **모든 항목 확정으로 §1.1로 이동**. 미합의 항목 없음.

### 1.3 D2 — RAG-friendly DTO 정본 스키마

```jsonc
{
  "schema_version": "1.0.0",
  "source": {
    "filename": "order.hwp",
    "format": "hwp",                 // hwp | hwpx
    "sha256": "ab12...",             // 원본 식별 / 중복 제거
    "page_count": 12,
    "section_count": 1,
    "extracted_at": "2026-04-27T10:00:00Z"
  },
  "blocks": [
    {
      "id": "b0001",                 // 안정적 고유 ID (재실행 dedup용)
      "type": "heading",             // heading|paragraph|table|image|list_item|header|footer|footnote|caption
      "level": 1,                    // heading 전용, 1~6 정규화
      "text": "주문서",
      "page": 1,
      "heading_path": ["주문서"]      // 자기 자신 포함 (heading의 경우)
    },
    {
      "id": "b0010",
      "type": "table",
      "page": 2,
      "heading_path": ["주문서", "1. 품목"],
      "markdown": "| SKU | 수량 |\n|-----|------|\n| A1 | 3 |\n| B2 | 5 |",
      "headers": ["SKU", "수량"],
      "rows": [{ "SKU": "A1", "수량": "3" }, { "SKU": "B2", "수량": "5" }],
      "merged_cells": []             // [{r,c,rowspan,colspan}, ...]
    },
    {
      "id": "b0020",
      "type": "image",
      "page": 3,
      "heading_path": ["주문서", "2. 첨부"],
      "asset_ref": "img_001",
      "caption": null,               // 별도 type:caption 블록으로 출력 (D29). 여기는 항상 null
      "alt": null,
      "width_mm": 80,
      "height_mm": 60,
      "near_text_before": "다음 로고를 사용한다.",
      "near_text_after": "사용 시 색상을 유지한다."
    },
    {
      "id": "b0021",
      "type": "caption",             // D29: 표/그림 캡션 별도 블록
      "ref_block_id": "b0020",
      "text": "그림 1. 회사 로고",
      "page": 3,
      "heading_path": ["주문서", "2. 첨부"]
    }
  ],
  "assets": {
    "img_001": {
      "mime": "image/png",
      "path": "order.assets/img_001.png",   // D4: extract 기본
      "sha256": "cd34...",
      "byte_size": 12345
    }
  }
}
```

**일부러 빼는 IR 디테일**: `line_segs`, `char_shapes`, `para_shape_id`, 글꼴/색상,
페이지 정의(여백/너비), 빈 문단. RAG 검색에 무용한 노이즈.

**텍스트 정규화** (D28): 모든 `text` 필드는 NFC 정규화. 추가 정규화는 downstream 책임.

**heading_path 누적 규칙** (D23): 이전 heading 블록을 stack에 push, 다음 heading이
같거나 낮은 level이면 pop. 모든 비-heading 블록은 현 시점 stack 복사를 `heading_path`로.

---

## 2. 워크스페이스 구조

본 저장소는 Cargo 워크스페이스로, `rhwp`(원본, 무수정 멤버)와 `rhwp-batch`(신규 멤버)
두 crate를 보유한다. **워크스페이스 디렉토리 트리, 빌드 명령은 [CLAUDE.md](CLAUDE.md) 참조**.

---

## 3. CLI 인터페이스 (마일스톤 추적용)

CLI 사양·옵션·종료 코드·예시는 사용자 정본 문서인 **[README.md "CLI 사용법"](README.md)** 에서
관리한다. 본 ROADMAP에서는 마일스톤별 구현 범위만 추적한다.

| 서브커맨드·옵션 | 도입 마일스톤 | 비고 |
|----------------|--------------|------|
| `to-json` (단일/디렉토리) | M2 | §8 산출물 |
| `to-json --image-mode {inline\|extract}`, `--image-dir`, `--pretty`, `--schema-version`, `--heading-style-map` | M2 | DTO는 M1에서 선행 도입. `--image-mode` 기본 **`extract`** (D4). |
| `fill` (단일) | M3 | §9 산출물 |
| `fill --overwrite` | M3 | D17 |
| `fill --on-missing-key error\|empty\|keep` | M3 Stage 4 | 기본 `error` (D14) |
| `fill --data-dir` / `--manifest` (배치) | M4 | §10 산출물, 양식 1회 파싱 |
| `fill --on-error fail-fast\|continue`, `--report` | M4 Stage 5 | 부분 실패 → exit code 4 |
| `fill --threads N` (병렬) | M4 Stage 5 | 기본 CPU 수 (D15), 측정 후 옵션화 |
| 공통 옵션 (`--log-format`, `--log-level`) | M2 | clap 골격에 포함 |
| 종료 코드 (0/1/2/3/4/10+) | M2 | M3·M4에서 새 코드 추가 시 README 갱신 동반 |

> README와 본 표가 어긋나면 **README가 정본**이다. ROADMAP은 마일스톤 진행 상태만 추적한다.

---

## 4. 배치 모드 (마일스톤 의의)

배치 모드(양식 1회 파싱 + JSON N회 채움)의 **알고리즘·전제·메모리 비용 분석**은
[DESIGN.md §5.4 "배치 모드의 IR 재사용"](DESIGN.md) 정본을 참조한다.

본 ROADMAP에서는 **왜 배치를 1급 시민으로 두는가**(운영 의의)만 기술한다.

- 양식 파싱(파일 IO + CFB 해독 + DocInfo 구축) **N회 → 1회**로 콜드 스타트 비용 분산.
- 양식이 클수록(수십 페이지, 폰트·스타일 다수) 효과 큼 → Airflow/K8s Job의 사용 패턴
  (한 잡에서 동일 양식으로 다건 처리)에 정확히 부합.
- 구현은 M4 (§10) 단계에서 진행하며, M3까지는 단일 채우기로 골격을 검증한다.

---

## 5. 마일스톤 개요 (수정판)

| MS | 제목 | 상태 | 완료일 | 주 산출물 |
|----|------|------|-------|----------|
| M1 | **JSON 스키마 + HWP→JSON** | ✅ 완료 | 2026-04-27 | DTO, 어댑터, 6종 통합 테스트 |
| M2 | **CLI 골격 + `to-json` 명령** | ✅ 완료 | 2026-04-27 | 바이너리 빌드, 단일/디렉토리 변환, 샘플 심볼릭 링크 |
| M3 | **양식 채우기 (`fill`, 텍스트)** | ✅ 완료 | 2026-04-27 | 마커 치환(JMESPath), 단일 파일 채우기, 8종 단위 테스트 |
| M4 | **배치 모드 + 표·이미지** | ✅ 완료 | 2026-04-27 | `--data-dir`/`--manifest`, 행 확장, 병렬 처리, 리포트 |
| M5 | **컨테이너화 + 잡 통합** | ✅ 완료 | 2026-04-27 | Dockerfile, K8s Job 템플릿, Airflow DAG 예시 |
| M6 | **HWPX 출력 (옵션)** | ⏸ 보류 | — | D5 결정: v1 제외. `serialize_hwpx` 안정화 후 별도 착수 |

> v0.1 대비 변경:
> - **M0 제거**: 준비·합의 단계는 본 ROADMAP 갱신과 결정 항목 표(§1)로 흡수.
>   설계 합의는 별도 마일스톤이 아니라 본 문서 변경 자체로 추적.
> - **M2 축소**: HTTP 서버 → CLI 골격 (1주 → 0.5주)
> - **M4에 배치 통합**: 양식 캐싱 효과를 1급으로
> - **M5 단순화**: HTTP 운영화(메트릭/HPA/LB) 제거 → 컨테이너 + 잡 매니페스트만 (1주 → 0.5주)
> - 합산 ~1.5주 단축 (M0 0.5주 + M2 0.5주 + M5 0.5주)

---

## 7. M1 — RAG-friendly JSON 스키마 + HWP→JSON 변환기

### 7.1 산출물
- `rhwp-batch/src/dto/` — RAG-friendly JSON 스키마 (블록 평탄 배열 + 메타데이터)
- `rhwp-batch/src/adapter/from_ir.rs` — `Document → DocumentJson` (블록 추출 + 정규화)
- `rhwp-batch/tests/convert_to_json.rs` — 샘플 HWP 변환 검증
- `dto_schema.md` — D2 스키마 정본 문서 (ROADMAP §1.3 인용 + 필드별 의미)

### 7.2 DTO 정본
정본 스키마는 [§1.3](#13-d2--rag-friendly-dto-정본-스키마) 참조. 본 절은 마일스톤
구현 범위만 추적한다.

### 7.3 단계 (4단계)

1. **Stage 1**: DTO 타입 정의 (`source` / `blocks` / `assets`) + 텍스트 블록 변환
   (`paragraph` / `heading`). NFC 정규화(D28) + heading_path 누적(D23) 적용.
2. **Stage 2**: 표 변환 (`type: table`). 병합셀(D25) + Markdown 직렬화 + `rows` 구조화 + 셀 다단 문단 `<br>` 결합(D26).
3. **Stage 3**: 이미지 추출 (`type: image`) + extract/inline 모드(D4 — **기본 extract**)
   + 인접 텍스트(`near_text_before/after`) 추출.
4. **Stage 4**: heading 정규화(D23, `--heading-style-map`) + 머리말/꼬리말/각주 분류(D24)
   + 표·그림 caption 별도 블록(D29) + `schema_version` 출력(D13).

### 7.4 종료 조건
- `cargo test -p rhwp-batch` 통과 (라이브러리 단위 테스트)
- `dto_schema.md` 작성 완료, ROADMAP §1.3과 일치
- 모든 출력 텍스트 NFC 정규화 검증 (테스트로 확인)
- 5종 샘플의 통합 검증은 **M2 종료 조건으로 이전** (CLI 진입점이 필요하기 때문)

---

## 8. M2 — CLI 골격 + `to-json` 명령

### 8.1 산출물
- `rhwp-batch` 바이너리 (clap 기반)
- `to-json` 서브커맨드 (단일 파일 + 디렉토리)
- 구조화 로깅 (`tracing` + `tracing-subscriber`)

### 8.2 의존성 (`rhwp-batch/Cargo.toml`)

```toml
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["json"] }
anyhow = "1"
base64 = "0.22"
walkdir = "2"
rhwp = { path = "../rhwp" }
```

> HTTP 관련 의존성(axum, tokio, tower-http) **제거**.

### 8.3 단계 (2단계)

1. **Stage 1**: clap 골격 + `--version`/`--help`/로그 포맷
2. **Stage 2**: `to-json` 서브커맨드 + 디렉토리 모드 + 종료 코드

### 8.4 종료 조건
- `rhwp-batch to-json sample.hwp -o out.json` 동작
- `rhwp-batch to-json --input-dir samples/ --output-dir out/` 동작
- `rhwp-batch/samples/from_rhwp/` (D19) 5종 변환 시 텍스트/표/이미지 누락 없음
- 100MB HWP 변환 시 메모리 사용 안정 (스트리밍은 불필요, 종료 시 해제)

---

## 9. M3 — 양식 채우기 (`fill`, 텍스트만)

### 9.1 산출물
- `fill` 서브커맨드 (단일 파일)
- 마커 파서 (`{{key}}` 외곽 + JMESPath 키, **D12** 참조)
- 채우기 정책 문서 `template_syntax.md`
- `--overwrite` 플래그 (**D17** 참조)

### 9.2 채우기 알고리즘 (텍스트 한정)

```text
1. parse_hwp(template_path) → Document
2. let mut core = DocumentCore::from_document(doc)
3. 모든 Paragraph.text를 순회하며 {{key}} 패턴 검출
   (CharShape 보존을 위해 split + 재삽입)
4. 각 매치에 대해:
   a. 해당 문단·offset에서 마커 길이만큼 delete
   b. core.insert_text_native(...)로 치환값 삽입
5. serialize_hwp(core.document()) → 출력 파일
```

### 9.3 단계 (4단계)

1. **Stage 1**: 마커 파싱·검출 (`{{` `}}` 외곽 + JMESPath 평가, 위치 리스트화)
2. **Stage 2**: 단일 문단 내 치환 (CharShape 분할 처리)
3. **Stage 3**: 머리말/꼬리말/표 셀 내부까지 확장
4. **Stage 4**: 누락 키 정책 (`--on-missing-key error|empty|keep`, **기본값 `error`** = D14)

### 9.4 종료 조건
- 양식 HWP 5종에 대해 마커 100% 치환
- CharShape 보존 (글꼴/크기/색상 유지) — PDF/SVG 비교
- 채워진 HWP를 한컴오피스에서 정상 열람

---

## 10. M4 — 배치 모드 + 표·이미지 채우기

### 10.1 산출물
- `--data-dir`, `--manifest` 배치 옵션
- 배치 러너 (`batch.rs`) — 양식 1회 파싱 + clone N회
- 표 행 동적 확장 마커 (`{{table:rows.items[].col}}`)
- 이미지 마커 (`{{image:logo}}`)
- 배치 리포트 (`--report report.json`)

### 10.2 마커 문법 예시

```jsonc
// 양식: 1행짜리 표, 셀에 {{table:rows.items[].name}} {{table:rows.items[].qty}}
{
  "rows": {
    "items": [
      { "name": "사과", "qty": 3 },
      { "name": "배",   "qty": 5 }
    ]
  }
}
// → 표가 자동으로 2행으로 확장
```

### 10.3 단계 (5단계)

1. **Stage 1**: 배치 러너 (양식 IR clone, 직렬 처리)
2. **Stage 2**: 표 마커 — 단일 행 확장
3. **Stage 3**: 표 마커 — 다열·다행 확장, 헤더 행 보존
4. **Stage 4**: 이미지 마커, base64 디코드 + `insert_picture_native`
5. **Stage 5**: 배치 리포트, 부분 실패 처리, `--threads N` 병렬 옵션

### 10.4 종료 조건
- 양식 1개 + JSON 100건 배치: 양식 파싱 시간 < 5% (전체 대비). **측정 = `tracing` span timing** (D20)
- 표 50행, 이미지 5장 양식 1건 채우기 < 2초
- `--on-error continue`로 일부 실패 시 나머지 정상 처리 + 종료 코드 4
- 결과 HWP가 한컴오피스에서 페이지 분할 정상

### 10.5 병렬 처리 검토 (Stage 5)

- `Document: Clone`이므로 양식을 N개 워커 스레드에 clone하여 분배 가능.
- `DocumentCore`가 `!Sync`이므로 **공유 불가** → 워커마다 인스턴스.
- 메모리: `(양식 크기) × 워커 수` 예상. 큰 양식은 워커 수 제한.
- 기본값은 **CPU 수** (`std::thread::available_parallelism`, **D15**). `--threads N` 미지정 시 자동.
- 우선 직렬 구현 후 측정 결과 보고 옵션화.

---

## 11. M5 — 컨테이너화 + 잡 통합

### 11.1 산출물
- `deploy/Dockerfile.batch` (multi-stage)
  - 빌드 스테이지: rust:1.x → `cargo build --release -p rhwp-batch`
  - 런타임 스테이지: debian-slim + 폰트(NanumGothic) + 바이너리
- `deploy/k8s/job-template.yaml` — 단일 작업 Job 매니페스트
- `deploy/k8s/airflow-dag-example.py` — Airflow에서 `KubernetesPodOperator`로 호출
- 운영 가이드 `ops_guide.md`
  - 양식 마운트 방법 (ConfigMap / PVC / S3 sync sidecar)
  - 입출력 볼륨 패턴
  - 로그 수집 (stdout JSON)

### 11.2 컨테이너 설계

```dockerfile
# 빌드
FROM rust:1-bookworm AS builder
WORKDIR /src
COPY . .
RUN cargo build --release -p rhwp-batch

# 런타임 (양식 미포함, 폰트만 동봉)
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y \
    fonts-nanum fonts-nanum-coding \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /src/target/release/rhwp-batch /usr/local/bin/
ENTRYPOINT ["/usr/local/bin/rhwp-batch"]
```

### 11.3 K8s Job 예시

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
          configMap: { name: rhwp-templates }      # 또는 PVC/CSI
        - name: input
          persistentVolumeClaim: { claimName: ... }
        - name: output
          persistentVolumeClaim: { claimName: ... }
```

### 11.4 단계 (3단계)
1. Dockerfile + 폰트 동봉 + 사내 레지스트리 푸시 스크립트
2. Job 템플릿 + Airflow DAG 예시
3. 운영 가이드 문서, 부하 시험 (배치 1000건 × 양식 1종)

### 11.5 종료 조건
- 컨테이너 1회 실행으로 100건 배치 처리 완료, exit code 0
- Airflow DAG 예시가 K8s Job을 생성·완료 추적
- 양식 변경 시 ConfigMap 갱신만으로 즉시 반영 (이미지 재빌드 불필요)

---

## 12. M6 — HWPX 출력 (옵션, 후순위)

### 12.1 진입 조건
- 사용자/파이프라인이 HWPX 출력을 명시적으로 요구
- 또는 기존 `serializer::hwpx` Stage 진척으로 충분히 안정화

### 12.2 옵션
- (a) 기존 `serialize_hwpx` Stage 보완 (별도 이슈로 본 프로젝트에 PR)
- (b) v1은 HWP만 지원, HWPX 입력은 받되 출력은 HWP로 통일

### 12.3 단계
D5(확정: HWPX 출력 v1 제외) 결정에 따라 별도 ROADMAP 후속 버전에서 상세화한다.

---

## 13. 위험 관리

| 위험 | 영향 | 완화 |
|------|------|------|
| `DocumentCore` 페이지네이션이 채우기마다 트리거 | 처리 시간 증가 | v1 감수 (D18). 측정 결과 임계 초과 시 v1.1에서 트랜잭션 API PR. |
| **rhwp 본체 PR이 v1 일정 안에 필요해질 가능성** | 일정·범위 위협 (무수정 원칙 충돌) | 페이지네이션 측정값으로 트리거 임계 사전 정의. 초과 시 즉시 v1 범위 축소 또는 일정 조정 결정. |
| 양식의 복잡한 컨트롤(필드/하이퍼링크/스타일)이 마커와 충돌 | 치환 실패 | 마커 문법을 충분히 고유하게 정의(`{{ }}`), 양식 작성 가이드. v1은 escape 미지원(D16). |
| HWPX 출력 미완성 | v1 범위 축소 | D5에서 명시적 제외 (v1 = HWP only) |
| 폰트 미설치로 페이지네이션 결과 차이 | 페이지 수 불일치 | 컨테이너에 NanumGothic 동봉, 폴백 로깅 |
| 양식 Document clone 메모리 비용 | 큰 양식 + 다워커 시 OOM | 워커 수 제한(D15 기본 CPU 수), `bin_data_content` 공유 최적화는 v2 |
| 외부 양식 경로 사용 시 권한·존재 여부 | 잡 실패 | 시작 시 `--template` 검증, 명확한 에러 메시지 + exit code 3 |
| 배치 일부 실패의 보고 누락 | 부분 데이터 생산 | `--report` 강제 권장, Airflow 후속 태스크에서 파싱 |
| 출력 파일 의도치 않은 덮어쓰기 | 데이터 유실 | 기본 거부 + `--overwrite` 명시 (D17), 종료 코드 3 |

---

## 14. 비-목표

사용자 입장의 비-목표 정본은 **[README.md "비-목표"](README.md)** 다. 본 ROADMAP에서는
**계획 범위 외**임을 명시하기 위해 다음 항목을 추가로 기록한다.

- 실시간 협업 편집, 사용자 관리, 멀티 테넌트 — 본 도구는 잡 단위 일회성 실행이 전제.
- 기존 `export-pdf` 등 상위 `rhwp` CLI 기능 재구현 — 필요 시 그쪽을 직접 호출한다.

(README의 비-목표와 본 표가 어긋나면 README가 정본이다.)

---

## 15. 다음 행동

M1~M5 코드는 모두 머지 완료(commit `9998680`). 남은 후속 작업:

| 항목 | 상태 | 비고 |
|------|------|------|
| `dto_schema.md` | ✅ 완료 | M1 §7.1 산출물. §1.3 정본 인용 + 필드별 의미 |
| `template_syntax.md` | ✅ 완료 | M3 §9.1 산출물. 마커 문법·on-missing-key 정책 |
| `ops_guide.md` | ✅ 완료 | M5 §11.1 산출물. 양식 마운트·볼륨·로그 수집 |
| fill·batch 통합 테스트 | 진행 중 | 프로그램적 양식 생성 기반 smoke test 도입 |
| 컨테이너 100건 부하 시험 (M5 §11.5) | 미실행 | 사내 K8s 환경 접근 시점에 실행 |
| 한컴오피스 열람 검증 (M3 §9.4) | 미실행 | Windows/한컴 환경 필요 |

> 결정 항목 D1~D29 모두 §1.1로 확정 완료. 추가 결정 항목 발생 시 본 §15에 누적 기록.
