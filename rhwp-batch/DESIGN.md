# DESIGN.md — rhwp-batch 기술 설계 (v1.0)

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
> | 2026-04-27 | 1.0 | M1~M5 구현 완료 반영. 실제 구현 기반으로 전면 갱신. |

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
│   ├── main.rs          # clap 진입점, 로깅 초기화, 종료 코드 매핑
│   ├── lib.rs           # 모듈 선언
│   ├── cli/
│   │   └── mod.rs       # Cli, Command, ToJsonArgs, FillArgs, ImageModeArg, OnMissingKey, OnError
│   ├── service/
│   │   └── mod.rs       # to_json_single, to_json_dir, fill_single, ToJsonConfig
│   ├── dto/
│   │   └── mod.rs       # DocumentJson, SourceJson, BlockJson(9 variants), MergedCell, AssetJson
│   ├── adapter/
│   │   ├── mod.rs       # pub re-exports
│   │   └── from_ir.rs   # convert(Document→DocumentJson), ConvertOptions, ImageMode, sha256_file
│   ├── template/
│   │   └── mod.rs       # fill_document, expand_table_markers, find_markers, resolve_value
│   ├── batch.rs         # run_batch, BatchConfig, BatchReport, items_from_data_dir, items_from_manifest
│   └── error.rs         # BatchError
├── tests/
│   └── convert_to_json.rs   # 통합 테스트 (6종)
└── samples/
    ├── from_rhwp/       # rhwp/samples/ 심볼릭 링크 (D19)
    └── templates/       # 양식 HWP 예시 (비어 있음, 사용자 추가)
```

---

## 3. 모듈 역할 상세

### 3.1 `cli/`

clap Derive 기반 CLI 정의. 비즈니스 로직 없음.

| 타입 | 설명 |
|------|------|
| `Cli` | 최상위. `--log-format`, `--log-level` 전역 옵션 |
| `ToJsonArgs` | `--input/--input-dir`, `--output/--output-dir`, `--image-mode`, `--image-dir`, `--pretty`, `--heading-style-map`, `--overwrite` |
| `FillArgs` | `--template`, `--data/--data-dir/--manifest`, `--output/--output-dir`, `--report`, `--on-missing-key`, `--on-error`, `--threads`, `--overwrite` |
| `ImageModeArg` | `extract` (기본, D4) / `inline` |
| `OnMissingKey` | `error` (기본, D14) / `empty` / `keep` |
| `OnError` | `stop` (기본) / `continue` |

### 3.2 `dto/`

`rhwp` IR을 외부에 직접 노출하지 않기 위한 **RAG-friendly JSON 스키마**.
`serde::Serialize/Deserialize`만 derive한다. IR 타입에 대한 의존 없음.

용도는 **RAG ingestion 입력**. 청크화·임베딩·벡터DB 적재는 downstream 책임.

```
DocumentJson
├── schema_version: "1.0.0"          # SemVer (D13)
├── source: SourceJson               # filename, format, sha256, section_count, extracted_at
├── blocks: Vec<BlockJson>           # ★ 평탄 배열 (청크 후보)
└── assets: HashMap<String, AssetJson>

BlockJson (#[serde(tag = "type", rename_all = "snake_case")])
├── Heading  { id, level: 1..=6, text, page, heading_path }
├── Paragraph { id, text, page, heading_path }
├── ListItem  { id, text, page, heading_path, level }
├── Table     { id, page, heading_path, markdown, headers, rows, merged_cells }
├── Image     { id, page, heading_path, asset_ref, alt?, width_mm, height_mm,
│              near_text_before?, near_text_after? }
├── Header    { id, text, page }     # D24: 항상 출력
├── Footer    { id, text, page }     # D24
├── Footnote  { id, text, ref_block_id?, page }
└── Caption   { id, text, ref_block_id, page, heading_path }  # D29

AssetJson { mime, path?, data_base64?, sha256, byte_size }   # D4: extract 기본
```

**불변식**:
- 모든 `text`는 NFC 정규화 (D28)
- `id`는 `"b{:04}"` 형식, 문서 내 순차 고유
- `heading_path`는 자기 자신 포함 (heading 블록) / 현재 스택 복사 (나머지)
- `assets` 키는 `Image.asset_ref`와 1:1 (동일 bin_data_id 재사용)

### 3.3 `adapter/from_ir.rs`

`Document IR → DocumentJson` 변환. `Converter` 구조체가 상태를 보유하며 섹션을 순회한다.

**핵심 함수**:
- `convert(doc, opts) → DocumentJson` — 공개 진입점
- `Converter::process_paragraph(para)` — 빈 문단 스킵, 스타일→heading 휴리스틱(D23), 컨트롤 분기
- `Converter::process_table(table)` — `cell_grid` 2D 매핑, 병합셀(D25), Markdown 직렬화(D26), 캡션(D29)
- `Converter::process_picture(pic)` — `bin_data_id` → `bin_data_content` 역참조, extract/inline(D4), 캡션(D29)
- `fill_near_text(blocks)` — Image 블록의 `near_text_before/after` 사후 채움 (2-pass)
- `build_markdown(grid)` — `|`, `\|` 이스케이프, `\n` → `<br>`(D26)

**이미지 bin_data 역참조**:
```
pic.image_attr.bin_data_id  (1-based)
  → doc_info.bin_data_list[id-1].storage_id
    → doc.bin_data_content.iter().find(|c| c.id == storage_id)
      → content.data.load()  // v0.7.19+/v0.8.0 BinDataBytes (Lazy|Loaded)
```

**heading 레벨 휴리스틱 (D23)**:
1. `--heading-style-map NAME=LEVEL` 사용자 override 우선
2. 스타일 `local_name`이 `제목N`, `개요N`, `Heading N` 패턴이면 N (1~6)
3. 해당 없으면 Paragraph로 처리

**시각적 HwpUnit 변환**: `1 HwpUnit = 1/7200 인치 → mm = u / 7200 × 25.4`

### 3.4 `service/`

CLI 인수를 받아 `adapter` + `rhwp` API를 조합하는 유스케이스 레이어.
파일 IO, 오류 전파, 종료 코드 매핑 담당.

| 함수 | 설명 |
|------|------|
| `to_json_single(input, output, cfg)` | 파일 1개 변환. sha256 계산 → parse_document → convert → JSON 출력 |
| `to_json_dir(input_dir, output_dir, cfg)` | walkdir로 HWP/HWPX 탐색, 각 파일에 to_json_single |
| `fill_single(template, data_path, output, ...)` | 양식 파싱 → JSON 로드 → template::fill_document → serialize_hwp → 출력 |

`ToJsonConfig`: CLI args에서 분리된 서비스 레이어 설정 구조체.

### 3.5 `template/`

양식 HWP 내 마커를 검출·치환한다. `rhwp` 직접 수정 접근법을 사용한다.

**마커 문법 (D3)**:
- `{{key}}` — 단순 값 치환 (JMESPath 표현식)
- `{{table:expr}}` — 표 행 동적 확장 (M4)
- `{{image:key}}` — 이미지 삽입 (v2, 현재 stub)

**텍스트 치환 구현**:
1. `find_markers(text)` — `{{...}}` 패턴 순차 파싱
2. `resolve_value(expr, data, on_missing)` — `jmespath::compile(expr).search(data)` 평가
3. `String::replace_range` 뒤→앞 순서로 적용 (인덱스 안정성)
4. `adjust_char_shapes` — CharShapeRef.start_pos(UTF-16 기준) 재조정
5. `para.line_segs.clear()` — 뷰어가 열 때 재계산 (D18 감수)

**표 행 확장 (D21, M4)**:
1. 표 내 template row 탐색 (`{{table:...}}` 마커 포함 행)
2. JMESPath 배열 표현식 평가 (예: `items[]`)
3. template row를 N개로 복제 (N = 배열 길이)
4. 각 복제 행에서 `last_field` 추출하여 해당 배열 요소 값으로 치환
5. `rebuild_grid(table)` — `cell_grid` 재구성, `row_count` 갱신

**CharShape 보존 방법**:
직접 모델 수정 + `section.raw_stream = None` 무효화로 serialize_hwp이 모델에서 재직렬화.
CharShapeRef.start_pos는 치환 길이 차이만큼 shift 적용.
DocumentCore 페이지네이션 트리거를 회피 (D18 v1 결정).

**키 표현식**: 마커 내부는 [JMESPath](https://jmespath.org/) 표준 (D12).
`jmespath 0.5` 크레이트 사용.

### 3.6 `batch.rs`

양식 1회 파싱 + `Document::clone()` N회 패턴의 배치 러너 (D21).

```text
parse_document(template_bytes) → Document        ← 1회
Arc<Document> 로 스레드 공유
for worker in 0..threads:
    for (data_path, output_path) in chunk:
        doc_clone = template_doc.clone()         ← 메모리 복제만
        fill_document(doc_clone, data, on_missing)
        serialize_hwp(filled) → 출력 파일
BatchReport { total, succeeded, failed, items }
```

- `run_batch(template, items, cfg)` — 진입점, Arc<Document> 공유, 스레드 분배
- `items_from_data_dir(data_dir, output_dir)` — `--data-dir` 모드 파일 목록 생성
- `items_from_manifest(manifest_path)` — `--manifest` JSON 파싱
- 부분 실패 → exit code 4 (D10)
- `--threads` 기본 `std::thread::available_parallelism` (D15)

---

## 4. 데이터 흐름

### 4.1 to-json (RAG ingestion 입력 생성)

```
파일 IO (read bytes)
  └─ sha256_file(bytes) → sha256
  └─ detect_format(bytes) → "hwp" | "hwpx"
  └─ parse_document(bytes) → Document (IR)
       └─ adapter::from_ir::convert(doc, opts) → DocumentJson (DTO)
            ├─ Converter::process_paragraphs (sections 순회)
            │    ├─ 빈 문단 스킵
            │    ├─ 스타일→heading 휴리스틱 (D23)
            │    ├─ Table → process_table (Markdown+rows+merged_cells, D25/D26)
            │    ├─ Picture → process_picture (asset extract/inline, D4)
            │    ├─ Header/Footer → type:header/footer (D24)
            │    └─ Footnote/Endnote → type:footnote
            └─ fill_near_text (Image near_text 2-pass 채움)
       └─ serde_json::to_writer (pretty 옵션, D27: 단일 문서 JSON)
       └─ extract 모드: 이미지 파일 <stem>.assets/ 저장
```

### 4.2 fill (단일)

```
파일 IO (template bytes)
  └─ parse_document(bytes) → Document
  └─ JSON data 로드 → serde_json::Value
  └─ template::fill_document(doc, data, on_missing)
       ├─ section.raw_stream = None (재직렬화 강제)
       ├─ 각 문단 {{key}} → resolve_value(JMESPath) → replace_range
       ├─ Table 컨트롤: expand_table (행 확장) → 셀 내 마커 치환
       └─ Header/Footer/Footnote 재귀 처리
  └─ serialize_hwp(filled_doc) → 출력 HWP bytes
  └─ 파일 쓰기 (--overwrite 미설정 시 존재 파일 거부, D17)
```

### 4.3 fill (배치)

```
parse_document(template) → Arc<Document>         ← 1회
  └─ N 항목을 threads 개 워커로 분배 (std::thread)
       └─ 각 워커: doc.clone() → fill_document → serialize_hwp → 파일 쓰기
  └─ BatchReport 집계 → --report JSON 출력
  └─ exit code: 0(전체 성공) / 4(부분 실패) / 1(전체 실패)
```

---

## 5. 오류 처리 전략

| 상황 | 처리 | 종료 코드 |
|------|------|----------|
| 입력 파일 없음·권한 오류 | 즉시 종료, tracing::error | 1 |
| HWP 파싱 실패 | 즉시 종료 | 1 |
| 인수 오류 (--input/--input-dir 누락 등) | 즉시 종료 | 2 |
| 출력 파일 이미 존재 (`--overwrite` 미설정) | 즉시 종료 | 3 (D17) |
| 배치 중 단일 항목 실패 (`--on-error continue`) | 항목 건너뜀, 리포트 기록 | 4 (D10) |
| 누락 키 (`--on-missing-key error`) | 즉시 종료 | 1 |
| 누락 키 (`empty` \| `keep`) | 빈 문자열 또는 마커 유지 | 0 |

> 종료 코드 표 정본은 [../README.md](../README.md).

---

## 6. 주요 의존성

| 크레이트 | 버전 | 용도 |
|---------|------|------|
| `rhwp` | path | HWP 파싱·직렬화 (`parse_document`, `serialize_hwp`) |
| `clap` | 4 | CLI 파싱 (Derive API) |
| `serde` / `serde_json` | 1 | DTO JSON 직렬화 |
| `tracing` / `tracing-subscriber` | 0.1/0.3 | 구조화 로그 (text\|JSON), span timing (D20) |
| `anyhow` | 1 | 오류 체인 |
| `thiserror` | 1 | `BatchError` 정의 |
| `base64` | 0.22 | 이미지 base64 인코딩 (inline 모드, D4) |
| `walkdir` | 2 | 디렉토리 순회 (`--input-dir`, `--data-dir`) |
| `jmespath` | 0.5 | 마커 키 JMESPath 평가 (D12) |
| `regex` | 1 | (의존성 준비, 현재 미사용) |
| `sha2` / `hex` | 0.10 / 0.4 | SHA-256 해시 (source.sha256, asset.sha256) |
| `unicode-normalization` | 0.1 | NFC 정규화 (D28) |

---

## 7. 성능 고려 사항

### 7.1 페이지네이션 비용 (D18)

`serialize_hwp` 는 `section.raw_stream = None`이면 모델에서 재직렬화한다.
`line_segs`를 비움으로써 한컴 뷰어가 파일 열 때 재레이아웃을 수행한다.
DocumentCore 경유(→ 페이지네이션 트리거) 없이 직접 모델을 수정하여 비용을 회피.
측정 도구: `tracing` span timing (D20). 임계 초과 시 v1.1에서 DocumentCore 트랜잭션 API PR 검토.

### 7.2 배치 모드 메모리 (D21)

`Document: Clone`이므로 양식 파싱 비용을 1회로 분산한다.
`Arc<Document>`를 통해 원본을 공유하고, 워커별로 `clone()` + 독립 수정.
워커 수 = min(CPU 수, 항목 수). 큰 양식(수십 MB)은 `--threads 1` 권장.

### 7.3 이미지 추출 (D4)

extract 모드에서 `bin_data_content.data`를 파일로 덤프한다.
동일 `bin_data_id`는 한 번만 기록 (`assets` HashMap 중복 체크).
inline 모드(`--image-mode inline`)는 base64 → JSON 내 `data_base64` 필드.
