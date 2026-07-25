# DESIGN.md — rhwp crate 분석 (v0.3)

> **분석 목적**: `rhwp-batch` 개발을 위한 `rhwp` crate 사전 조사.
> `rhwp`에서 **재사용할 수 있는 자산**과 **rhwp-batch가 새로 만들어야 할 부분**을
> 식별하는 데 초점을 둔다.
>
> **다른 문서**:
> - 워크스페이스 설계 개요: [../DESIGN.md](../DESIGN.md)
> - rhwp-batch 설계: [../rhwp-batch/DESIGN.md](../rhwp-batch/DESIGN.md)
> - 사용자 가이드: [../README.md](../README.md)
> - 마일스톤·결정·위험: [../ROADMAP.md](../ROADMAP.md)
> - 작업 계약·주의사항: [../CLAUDE.md](../CLAUDE.md)
>
> | 일자 | 버전 | 변경 |
> |------|------|------|
> | 2026-04-27 | 0.1 | 최초 작성 (HTTP 서버 가정) |
> | 2026-04-27 | 0.2 | CLI/배치/양식 외부 경로 모델로 전환 |
> | 2026-04-27 | 0.3 | 4문서 역할 분리, 링크·표현 정정, rhwp/ 폴더로 이동 |

---

## 1. crate 한눈에 보기

| 항목 | 내용 |
|------|------|
| 언어 | Rust 2021 (`rhwp` v0.8.0) |
| 빌드 산출물 | `cdylib` + `rlib` (네이티브 lib + WASM lib) + `rhwp` CLI |
| 핵심 의존성 | `cfb` (OLE), `flate2`, `zip`, `quick-xml`, `encoding_rs`, `image`, `wasm-bindgen` |
| 네이티브 전용 | `svg2pdf`, `usvg`, `pdf-writer`, `subsetter`, `ttf-parser` |
| **serde / serde_json** | **포함** (v0.8.0 기준; IR JSON 스키마는 여전히 rhwp-batch DTO가 담당) |
| HTTP 프레임워크 | 없음 — rhwp-batch CLI가 잡 러너에 의해 직접 실행됨 |

이 crate는 본질적으로 **HWP 포맷 처리 라이브러리 + 웹/데스크톱 뷰어/에디터**다.
`rhwp-batch`는 이 라이브러리를 무수정으로 의존하며, 신규 코드를 일절 추가하지 않는다.

---

## 2. 디렉토리 지도 (rhwp-batch 관점에서 중요한 것만)

```
src/
├── lib.rs                  # 공개 API: parse_document / serialize_document / DocumentCore
├── main.rs                 # rhwp CLI 진입 (호출 패턴 참고용)
├── model/                  # IR 타입 정의 (Document/Section/Paragraph/Control/Table/...)
├── parser/                 # 입력 파이프라인
│   ├── mod.rs              # parse_hwp() — HWP/CFB
│   ├── hwpx/               # parse_hwpx() — HWPX/ZIP+XML
│   ├── doc_info.rs         # 글꼴/스타일/BinData 메타
│   └── body_text/          # 문단/컨트롤 파싱
├── serializer/             # 출력 파이프라인
│   ├── cfb_writer.rs       # serialize_hwp()  ✓ 완료 (라운드트립)
│   └── hwpx/               # serialize_hwpx() ⚙ Stage 0-1 (부분)
├── document_core/          # ★ 편집 엔진 (rhwp-batch가 직접 사용)
│   ├── mod.rs              # DocumentCore — "WASM/PyO3/MCP 어댑터 공용" 명시
│   ├── commands/
│   │   ├── text_editing.rs # insert_text_native / split_paragraph_native
│   │   └── object_ops.rs   # create_table_native / insert_picture_native
│   └── queries/            # 읽기 질의
├── renderer/               # SVG/PDF/HTML/canvas (rhwp-batch는 사용 안 함)
└── wasm_api.rs             # WASM 어댑터 — rhwp-batch는 사용 안 하나 호출 패턴 참고
```

**핵심 포인트**: `document_core/` 는 처음부터 *어댑터 중립*으로 설계됐다
([src/document_core/mod.rs:1-4](src/document_core/mod.rs#L1-L4)).
rhwp-batch는 `wasm_api`를 우회하고 `DocumentCore` + `*_native()` 함수들을 직접 호출한다.

---

## 3. 입력 파이프라인 (HWP/HWPX → IR)

### 3.1 진입점

| 파일 | 함수 | 입력 | 출력 |
|------|------|------|------|
| [src/parser/mod.rs:115](src/parser/mod.rs#L115) | `parse_hwp(&[u8])` | HWP 바이트 | `Document` |
| [src/parser/hwpx/mod.rs:63](src/parser/hwpx/mod.rs#L63) | `parse_hwpx(&[u8])` | HWPX 바이트 | `Document` |
| [src/parser/mod.rs](src/parser/mod.rs) | `parse_document(&[u8])` | 자동 감지 | `Document` |

### 3.2 처리 단계 (HWP)

1. CFB(OLE) 컨테이너 해독 → `cfb_reader::CfbReader`
2. `/FileHeader` (256B) → `header::parse_file_header()`
3. `/DocInfo` → 글꼴/스타일/BinData 메타 테이블 구축
4. `/BodyText/Section{N}` → 문단·컨트롤 트리
5. `/BinData/*` → 이미지 바이너리 로드 (BMP/PNG/GIF 자동 감지)
6. 자동 번호 할당 (페이지/표/그림/수식)

### 3.3 처리 단계 (HWPX)

1. ZIP 컨테이너 해독 → `reader::HwpxReader`
2. `Contents/content.hpf` → 섹션 목록 + BinData 메타
3. `Contents/header.xml` → DocInfo 매핑
4. `Contents/section{N}.xml` → 섹션별 IR
5. `BinData/`, `Media/` → 이미지

> 두 경로 모두 **동일한 `Document` IR**을 생성한다. rhwp-batch는 입력 포맷에
> 무관하게 단일 처리 경로를 가질 수 있다.

---

## 4. 중간 표현(IR) 모델

### 4.1 최상위

```rust
// src/model/document.rs:24
pub struct Document {
    pub header: FileHeader,                   // 버전, 압축 플래그
    pub doc_properties: DocProperties,        // 페이지/표/그림 시작번호
    pub doc_info: DocInfo,                    // 글꼴, 스타일, BinData 메타
    pub sections: Vec<Section>,               // 본문 (구역별)
    pub preview: Option<Preview>,             // 썸네일
    pub bin_data_content: Vec<BinDataContent>,// 이미지 바이너리
    pub extra_streams: Vec<(String, Vec<u8>)>,// 라운드트립 보존용
}
```

### 4.2 문단 / 컨트롤

```rust
// src/model/paragraph.rs:5
pub struct Paragraph {
    pub text: String,                  // UTF-16 → String
    pub char_count: u32,
    pub para_shape_id: u16,
    pub style_id: u8,
    pub controls: Vec<Control>,        // 표/그림/필드/주석 등
    pub char_shapes: Vec<CharShapeRef>,
    pub line_segs: Vec<LineSeg>,
}

// src/model/control.rs:14 (총 18종 variant)
pub enum Control {
    SectionDef(Box<SectionDef>),
    ColumnDef(ColumnDef),
    Table(Box<Table>),
    Shape(Box<ShapeObject>),
    Picture(Box<Picture>),
    Equation(Box<Equation>),
    Footnote(Box<Footnote>),
    Hyperlink(Hyperlink),
    // ...
}
```

### 4.3 직렬화 적합성 분석

| 항목 | 현재 상태 | rhwp-batch 대응 |
|------|----------|-----------------|
| `derive(Serialize/Deserialize)` | **없음** | rhwp-batch에서 DTO 레이어로 우회 |
| `Vec<u8>` 바이너리 (이미지 등) | 다수 | DTO에서 base64 인코딩 또는 별도 첨부 |
| `Box<T>` 사용 | 다수 | serde 기본 지원, 문제 없음 |
| 라운드트립 보존 필드 (`raw_data`, `extra_streams`) | 있음 | DTO에서 생략 — 양식 원본은 직렬화 불필요 |
| `enum Control` (18 variant) | 태그 없음 | DTO에서 `#[serde(tag="type")]` 별도 정의 |

> rhwp 모델 타입에 일괄 `derive(Serialize, Deserialize)`를 추가하는 것은
> 무수정 원칙에 위배된다. rhwp-batch는 **별도 DTO + IR↔DTO 변환기**를 두는
> Hexagonal 방식을 채택한다. 상세는 [../rhwp-batch/DESIGN.md §3.2](../rhwp-batch/DESIGN.md).

---

## 5. 편집 엔진 (`DocumentCore`)

rhwp-batch의 "JSON → HWP" 경로의 **핵심 자산**이다.

### 5.1 설계 의도

[src/document_core/mod.rs:1-4](src/document_core/mod.rs#L1-L4):
> "WASM/PyO3/MCP 등 어떤 어댑터에서도 독립적으로 사용할 수 있다."

→ rhwp-batch 어댑터는 1급 시민이다. 추가 작업 없이 바로 `use` 가능.

### 5.2 rhwp-batch에서 쓸 native 메서드

| 메서드 | 위치 | 용도 |
|--------|------|------|
| `insert_text_native(sec, para, off, text)` | [text_editing.rs:14](src/document_core/commands/text_editing.rs#L14) | 텍스트 삽입 |
| `split_paragraph_native(...)` | text_editing.rs | 엔터 (문단 분리) |
| `create_table_native(sec, para, off, rows, cols)` | [object_ops.rs:512](src/document_core/commands/object_ops.rs#L512) | 표 삽입 |
| `create_table_ex_native(json_opts)` | [object_ops.rs:809](src/document_core/commands/object_ops.rs#L809) | 표 + 열너비 |
| `insert_picture_native(...)` | [object_ops.rs:1039](src/document_core/commands/object_ops.rs#L1039) | 그림 삽입 |
| `insert_text_in_cell_native(...)` | text_editing.rs | 셀 내 텍스트 |

> `wasm_api.rs`의 251개 메서드는 대부분 `DocumentCore::*_native()`의 얇은
> 래퍼다. rhwp-batch는 wasm_api를 거치지 않고 `DocumentCore`만 의존한다.

### 5.3 부수 효과

`*_native()` 호출은 다음을 자동으로 수행한다:
- `raw_stream = None` 무효화 (재직렬화 강제)
- `reflow_paragraph()` (line_segs 재계산)
- `recompose_paragraph()`
- `paginate_if_needed()` (페이지 재분할)

rhwp-batch의 일괄 채우기(batch insert) 시 **매 호출마다 페이지네이션이 트리거**되어
오버헤드가 클 수 있다. 페이지네이션을 지연시키는 "트랜잭션 모드"가 필요할
수 있음 → ROADMAP M4에서 다룬다.

### 5.4 배치 모드의 IR 재사용 (★)

`Document` 타입은 `#[derive(Clone)]` ([src/model/document.rs:24](src/model/document.rs#L24))이다.
rhwp-batch는 다음 흐름으로 양식 파싱 비용을 N건 작업에 1회로 분산한다:

```text
1. parse_hwp(template_bytes) → Document               ← 1회 (CFB 해독, DocInfo, BodyText 전부)
2. for each json in inputs:
     a. let doc = template.clone()                     ← 메모리 복제만 (파싱 없음)
     b. let mut core = DocumentCore::from_document(doc)
     c. apply_template_data(&mut core, json)           ← 마커 치환·표·이미지
     d. serialize_hwp(core.document()) → 출력 파일
```

**전제**:
- `DocumentCore` 자체는 페이지네이션 캐시 등을 보유하므로 **재사용 불가** —
  매 작업마다 `Document` clone에서 새로 구성해야 한다.
- 큰 양식의 `bin_data_content` clone 비용이 누적될 수 있음 → `Arc` 공유는 v2 후보.

---

## 6. 출력 파이프라인 (IR → HWP/HWPX)

### 6.1 진입점

| 파일 | 함수 | 상태 |
|------|------|------|
| [src/serializer/cfb_writer.rs:23](src/serializer/cfb_writer.rs#L23) | `serialize_hwp(&Document)` | **완료** (라운드트립 검증됨) |
| [src/serializer/hwpx/mod.rs:40](src/serializer/hwpx/mod.rs#L40) | `serialize_hwpx(&Document)` | **Stage 0-1** (부분 동작) |
| [src/serializer/mod.rs](src/serializer/mod.rs) | `serialize_document()` | trait 추상 |

### 6.2 라운드트립 보장

- HWP: 원본 스트림(`raw_stream`, `raw_data`, `extra_streams`)을 IR에 보존하여
  편집되지 않은 영역은 **바이트 단위로 동일**하게 재출력 가능.
  → 양식 HWP의 머리말/꼬리말/스타일 등은 손실 없이 유지된다.
- HWPX: 동일 전략이지만 XML 직렬화 단계가 **미완성** (현재 Stage 0-1).

### 6.3 알려진 제약

- `extra_streams`에 라운드트립용 미해석 스트림이 보존됨. 깊은 편집을
  하면 일관성 위반 가능 → "텍스트 치환 + 표/이미지 삽입" 수준만 지원하고
  그 외는 양식에 의존한다.
- HWPX 직렬화는 일부 컨트롤 타입에서 미완성. rhwp-batch v1은 **HWP 우선**,
  HWPX는 보완 후 도입을 권장.

---

## 7. rhwp CLI (호출 패턴 참고)

[src/main.rs](src/main.rs)에 14개 서브커맨드 존재:
- `info` / `dump` / `dump-pages` / `dump-records` / `diag` — 진단
- `export-svg` / `export-pdf` / `thumbnail` — 출력
- `convert` — 배포용 → 편집 가능 HWP
- `gen-table` / `test-shape` / `test-caption` / `test-field` — 라운드트립 테스트
- `ir-diff` — HWPX↔HWP 비교

rhwp-batch 구현 시 `gen-table` ([src/main.rs](src/main.rs))은 "표를 새로 만들고
저장하는" 흐름의 *살아있는 레퍼런스*가 된다.

---

## 8. 이미지 처리

### 8.1 추출 (HWP → JSON)

- [src/parser/mod.rs:698-758](src/parser/mod.rs#L698) `load_bin_data_content()`
- 압축 해제 + OLE prefix 처리
- `image` 크레이트로 BMP/PNG 디코드 가능

### 8.2 삽입 (JSON → HWP)

- `DocumentCore::insert_picture_native()` 사용
- 입력: `image_data: &[u8]`, `width`, `height`, 위치, 배치 옵션
- BinData 슬롯 자동 할당 + Picture 컨트롤 생성

### 8.3 JSON 이미지 운반 정책 (결정 필요 → ROADMAP D4)

| 방식 | 장점 | 단점 |
|------|------|------|
| base64 인라인 | 단일 파일, 단순 | 크기 33% 증가, 메모리 사용↑ |
| 별도 첨부 (디렉토리) | 효율적 | 운반 규약 추가 필요 |
| URL 참조 | 가장 가벼움 | 외부 의존, rhwp-batch가 페치해야 |

---

## 9. rhwp-batch 재사용 자산 매핑

| rhwp-batch 요구사항 | 재사용 가능한 rhwp 코드 | rhwp-batch 신규 작성 |
|--------------------|------------------------|---------------------|
| HWP/HWPX 파싱 | `parser::parse_document()` | — |
| IR → JSON 직렬화 | — | DTO + 변환기 |
| JSON → IR 적용 | — | DTO + 변환기 |
| 양식 HWP 로드 | `parser::parse_hwp()` | — |
| 양식 IR 재사용 (배치) | `Document: Clone` | 배치 러너 |
| 텍스트 채우기 | `DocumentCore::insert_text_native` | — |
| 표 채우기 | `create_table_*_native` | 행 단위 채우기 헬퍼 |
| 이미지 삽입 | `insert_picture_native` | base64 디코드 래퍼 |
| HWP 저장 | `serialize_hwp()` | — |
| HWPX 저장 | `serialize_hwpx()` (부분) | v2 연기 |
| CLI 진입점 | — | clap 기반 CLI |
| 양식 외부 경로 로드 | `std::fs` | 경로 인자 처리 |
| 폰트 / 렌더링 | `renderer::*` | 불필요 (rhwp-batch는 미사용) |

---

## 10. 위험·미정 항목

위험 관리 표와 결정 대기 항목(D2~D11)은 **[../ROADMAP.md §13](../ROADMAP.md), [§1](../ROADMAP.md)** 으로
일원화한다. 본 문서의 기술 분석에서 파생된 항목만 아래에 요약한다.

- **JSON 스키마 모양** (D2): IR 투명 노출 X, **RAG-friendly DTO** 분리 (§4.3 근거, 정본은 [../ROADMAP.md §1.3](../ROADMAP.md)).
- **양식 채우기 슬롯** (D3): 마커 문법 vs 인덱스 vs HWP 필드 — 마커 우세.
- **HWPX 직렬화 미완성** (D5): v1은 HWP only (§6.2 근거).
- **페이지네이션 비용**: `*_native()` 매 호출마다 트리거 (§5.3 근거).
- **폰트 의존성**: 페이지네이션이 폰트 메트릭에 의존 — 컨테이너에 NanumGothic 동봉.
- **동시성**: `DocumentCore`는 `RefCell` → `!Sync`. 워커마다 인스턴스 (§5.4 근거).
