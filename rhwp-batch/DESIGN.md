# DESIGN.md — rhwp-batch 기술 설계 (v0.1)

> **목적**: `rhwp-batch` CLI crate의 내부 구조·모듈 설계·데이터 흐름을 기술한다.
>
> **다른 문서**:
> - 워크스페이스 전체 설계 개요: [../DESIGN.md](../DESIGN.md)
> - `rhwp` crate 분석: [../rhwp/DESIGN.md](../rhwp/DESIGN.md)
> - 사용자 가이드: [../README.md](../README.md)
> - 마일스톤·결정·위험: [../ROADMAP.md](../ROADMAP.md)
> - 작업 계약: [../CLAUDE.md](../CLAUDE.md)
>
> | 일자 | 버전 | 변경 |
> |------|------|------|
> | 2026-04-27 | 0.1 | 최초 작성 (스켈레톤 + 결정 항목 D1~D22 기준) |

---

## 1. crate 한눈에 보기

| 항목 | 내용 |
|------|------|
| crate 이름 | `rhwp-batch` |
| 산출물 | 바이너리 `rhwp-batch` + lib `rhwp_batch` |
| 주 역할 | HWP/HWPX → JSON 변환, JSON + 양식 HWP → 채워진 HWP 생성 |
| 의존 crate | `rhwp` (무수정, 로컬 경로) |
| 실행 환경 | Linux, Airflow `KubernetesPodOperator` / K8s `Job` |

---

## 2. 디렉토리 구조

```
rhwp-batch/
├── Cargo.toml
├── src/
│   ├── main.rs          # clap 진입점, 로깅 초기화
│   ├── lib.rs           # 모듈 선언
│   ├── cli/             # CLI 인터페이스 (clap Derive)
│   │   └── mod.rs       # Cli, Command, ToJsonArgs, FillArgs
│   ├── service/         # 변환·생성 비즈니스 로직
│   │   └── mod.rs       # to_json_service, fill_service (M1-M3)
│   ├── dto/             # JSON 스키마 타입 (serde)
│   │   └── mod.rs       # DocumentJson, SectionJson, ParagraphJson, AssetJson
│   ├── adapter/         # IR ↔ DTO 변환기
│   │   └── mod.rs       # from_ir::convert (M1), to_ir::apply (M3)
│   ├── template/        # 양식 마커 파싱·치환
│   │   └── mod.rs       # parse_markers, substitute (M3)
│   ├── batch.rs         # 배치 러너 (M4)
│   └── error.rs         # BatchError
├── tests/               # 통합 테스트 (M1~)
└── samples/
    └── templates/       # 테스트용 양식 HWP 예시
```

---

## 3. 모듈 역할 상세

### 3.1 `cli/`

clap Derive 기반 CLI 정의. 비즈니스 로직 없음. 두 서브커맨드를 노출한다.

| 서브커맨드 | 구현 마일스톤 | 핵심 인수 |
|-----------|--------------|----------|
| `to-json` | M2 | `--input`, `--input-dir`, `--output`, `--output-dir` |
| `fill`    | M3 (단일) / M4 (배치) | `--template`, `--data`, `--data-dir`, `--manifest`, `--output`, `--output-dir` |

### 3.2 `dto/`

`rhwp` IR을 외부에 직접 노출하지 않기 위한 **RAG-friendly JSON 스키마**.
`serde::Serialize/Deserialize`만 derive한다. IR 타입(`Document`, `Paragraph` 등)에 대한 의존 없음.

용도는 **RAG ingestion 입력**. 청크화·임베딩·벡터DB 적재까지 downstream 책임이며,
본 DTO는 의미 단위 블록과 메타데이터만 제공한다.

```
DocumentJson
├── schema_version: String           # SemVer (D13). 초기 "1.0.0"
├── source: SourceJson               # filename, format, sha256, page_count, extracted_at
├── blocks: Vec<BlockJson>           # ★ 평탄 배열 (청크 후보)
└── assets: HashMap<String, AssetJson>

BlockJson (#[serde(tag = "type")])
├── Heading      { id, level: 1..=6, text, page, heading_path }
├── Paragraph    { id, text, page, heading_path }
├── ListItem     { id, text, page, heading_path, level }
├── Table        { id, page, heading_path, markdown, headers, rows, merged_cells }
├── Image        { id, page, heading_path, asset_ref, alt, w_mm, h_mm,
│                 near_text_before, near_text_after }
├── Header       { id, text, page: None }     # D24
├── Footer       { id, text, page: None }     # D24
├── Footnote     { id, text, ref_block_id, page }
└── Caption      { id, text, ref_block_id, page, heading_path }   # D29

AssetJson  { mime, path | data_base64, sha256, byte_size }   # D4: extract 기본
```

**정본**: 스키마 형상은 [../ROADMAP.md §1.3](../ROADMAP.md) 정본을 따른다. 본 문서는
`serde` 매핑 관점만 기술.

**불변식**:
- 모든 `text`는 NFC 정규화 (D28)
- `id`는 입력이 같으면 같은 값 (안정 ID, 재실행 dedup)
- `heading_path`는 자기 자신을 포함 (heading 블록의 경우)
- `assets`의 키는 `Image.asset_ref`와 1:N (한 이미지를 여러 블록이 참조 가능)

### 3.3 `adapter/`

IR → DTO (`from_ir`, M1)와 DTO → IR 적용 (`to_ir`, M3) 두 방향의 변환기.
`rhwp::Document` / `rhwp::DocumentCore` 에 의존하지만 JSON 타입에는 의존하지 않는다.

### 3.4 `service/`

CLI 인수를 받아 `adapter` + `rhwp` API를 조합하는 유스케이스 레이어.
파일 IO, 오류 전파, 종료 코드 매핑을 담당한다.

### 3.5 `template/`

양식 HWP 내 마커(`{{key}}`, `{{table:...}}`, `{{image:...}}`)를 검출·치환한다.
`DocumentCore::insert_text_native` 등 `rhwp` 편집 API를 내부에서 호출한다.

**키 표현식**: 마커 외곽(`{{`, `}}`, prefix `table:`/`image:`)은 자체 정의이나
**키 부분은 [JMESPath](https://jmespath.org/) 표준** (D12)을 그대로 사용한다.
구현은 `jmespath` 크레이트(또는 동등 라이브러리)로 평가하며, 자체 점 표기 파서를
작성하지 않는다.

- `{{customer.name}}` → JMESPath `customer.name`
- `{{table:rows.items[].sku}}` → prefix `table:` + JMESPath `rows.items[].sku` (projection)
- `{{image:logo}}` → prefix `image:` + JMESPath `logo`

마커 escape는 v1 미지원 (D16). 양식에 `{{` 리터럴 등장 금지를 양식 가이드에서 강제.

### 3.6 `batch.rs`

양식 1회 파싱 + `Document::clone()` N회 패턴의 배치 러너.
직렬(M4 Stage 1) → 병렬(`rayon`, M4 Stage 5) 로 단계적 확장.

```text
parse_hwp(template) → Document          ← 1회
for each item:
    doc = template.clone()              ← 메모리 복제만
    core = DocumentCore::from_document(doc)
    apply(&mut core, data)
    serialize_hwp(core.document()) → 출력
```

---

## 4. 데이터 흐름

### 4.1 to-json (RAG ingestion 입력 생성)

```
파일 IO
  └─ rhwp::parse_document(&bytes) → Document (IR)
       └─ adapter::from_ir::convert(&doc, opts) → DocumentJson (DTO)
            ├─ 블록 추출 (paragraph/heading/table/image/header/footer/footnote/caption)
            ├─ NFC 정규화 (D28)
            ├─ heading_path 누적 (D23, --heading-style-map override)
            ├─ 표 → Markdown + rows + merged_cells (D25/D26)
            └─ 이미지 → asset 분리 (D4 기본 extract) + near_text_before/after
       └─ serde_json::to_writer → JSON 파일
       └─ asset extract 모드면 별도 디렉토리에 이미지 저장
```

### 4.2 fill (단일)

```
파일 IO (template HWP)
  └─ rhwp::parse_hwp(&bytes) → Document
       └─ DocumentCore::from_document(doc)
            └─ template::substitute(&mut core, &data_json)
                 └─ rhwp::serialize_hwp(core.document()) → 출력 HWP
```

### 4.3 fill (배치)

```
파일 IO (template HWP)
  └─ parse_hwp → Document (template)
       └─ for each JSON:
            template.clone() → doc
            DocumentCore::from_document(doc)
            template::substitute(...)
            serialize_hwp → 출력 HWP[i]
       └─ BatchReport → --report JSON
```

---

## 5. 오류 처리 전략

| 상황 | 처리 | 종료 코드 |
|------|------|----------|
| 입력 파일 없음·권한 오류 | 즉시 종료, 메시지 출력 | 3 |
| HWP 파싱 실패 | 즉시 종료 | 1 |
| JSON 역직렬화 실패 | 즉시 종료 | 2 |
| 배치 중 단일 항목 실패 (`--on-error continue`) | 항목 건너뜀, 리포트에 기록 | 4 (부분 실패) |
| 누락 키 (`--on-missing-key error`) | 즉시 종료 | 2 |
| 누락 키 (`empty`\|`keep`) | 빈 문자열 또는 마커 유지 | 0 |

> 종료 코드 표 정본은 [../README.md](../README.md).

---

## 6. 주요 의존성

| 크레이트 | 용도 |
|---------|------|
| `rhwp` (path) | HWP 파싱·편집·직렬화 |
| `clap 4` | CLI 파싱 |
| `serde / serde_json` | DTO JSON 직렬화 |
| `tracing / tracing-subscriber` | 구조화 로그 (text \| JSON), span timing 측정 (D20) |
| `anyhow` | 오류 체인 |
| `thiserror` | `BatchError` 정의 |
| `base64` | 이미지 base64 인코딩 |
| `walkdir` | 디렉토리 순회 (`--input-dir`, `--data-dir`) |
| `jmespath` (또는 동등) | 마커 키 평가 (D12) |
