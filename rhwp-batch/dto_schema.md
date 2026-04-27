# dto_schema.md — RAG-friendly JSON 스키마 정본

> **역할**: `to-json` 출력 JSON 스키마의 정본 문서.
> **권한 우선순위**: [ROADMAP.md §1.3](../ROADMAP.md) > 본 문서 > 코드.
> ROADMAP.md §1.3와 본 문서가 어긋나면 ROADMAP가 정본이며, 본 문서를 갱신한다.

---

## 1. 최상위 구조

```jsonc
{
  "schema_version": "1.0.0",
  "source":   { ... },     // 원본 메타
  "blocks":   [ ... ],     // RAG 청크 후보 평탄 배열
  "assets":   { ... }      // (선택) 이미지 자산
}
```

| 필드 | 타입 | 필수 | 의미 |
|------|------|:---:|------|
| `schema_version` | string | ✓ | SemVer (D13). 초기 `1.0.0`. 메이저 불일치 시 consumer 거부 의무. |
| `source` | object | ✓ | 원본 식별·중복 제거 메타 |
| `blocks` | array | ✓ | 평탄 블록 배열. 순서 = 본문 흐름 순서 |
| `assets` | object | — | `assets[asset_ref]` 형태. 빈 객체면 직렬화에서 제외 |

---

## 2. `source`

| 필드 | 타입 | 의미 |
|------|------|------|
| `filename` | string | CLI에 전달된 입력 파일명 (basename) |
| `format` | string | `"hwp"` 또는 `"hwpx"` (D1: HWPX 입력은 받음, 출력은 D5에 따라 v1 제외) |
| `sha256` | string | 원본 바이트 SHA-256 hex. 파이프라인 dedup 키로 사용 가능 |
| `section_count` | number | 본문 섹션 수 |
| `extracted_at` | string | ISO 8601 UTC (`YYYY-MM-DDTHH:MM:SSZ`) |

**중복 제거 권장 키**: `(sha256, schema_version)`.

---

## 3. `blocks` 공통 규칙

- **block id (`id`)**: `b0001`, `b0002`, ... 안정적 4자리 zero-padded. 동일 입력 재실행 시 동일 ID.
- **`page`**: v1에서는 항상 `null` (페이지네이션 비용 D18 절약). v1.1 이후 채울 수 있음.
- **`heading_path`**: 가장 최근의 heading 스택 누적 (D23). `Heading` 자신은 자기 자신 포함.
- **텍스트 정규화**: 모든 `text` 필드는 NFC (D28). 추가 정규화는 downstream chunker.
- **`type` discriminator**: serde tagged enum. v2 이후 새 type 추가는 minor SemVer.

### 3.1 블록 종류

| `type` | 추가 필드 | 비고 |
|--------|----------|------|
| `heading` | `level` (1~6, D23 정규화), `text`, `heading_path` | `heading_path` 마지막 = 자기 자신 |
| `paragraph` | `text`, `heading_path` | 빈 텍스트는 출력하지 않음 |
| `list_item` | `text`, `heading_path`, `level` | v1 비활성 (paragraph로 폴백). v2 활성화 |
| `table` | `markdown`, `headers`, `rows`, `merged_cells` | §4 참조 |
| `image` | `asset_ref`, `width_mm`, `height_mm`, `alt`, `near_text_before`, `near_text_after` | §5 참조 |
| `header` | `text` | 머리말. D24: 항상 출력, downstream 필터링 책임 |
| `footer` | `text` | 꼬리말. D24 동일 |
| `footnote` | `text`, `ref_block_id?` | Endnote도 footnote로 통합 출력 |
| `caption` | `text`, `ref_block_id` (필수) | D29. 표·그림 캡션을 별도 블록으로 |

---

## 4. `table` 상세

```jsonc
{
  "type": "table",
  "id": "b0010",
  "page": null,
  "heading_path": ["주문서", "1. 품목"],
  "markdown": "| SKU | 수량 |\n|-----|------|\n| A1 | 3 |",
  "headers": ["SKU", "수량"],
  "rows": [{ "SKU": "A1", "수량": "3" }],
  "merged_cells": [{ "r": 0, "c": 0, "rowspan": 2, "colspan": 1 }]
}
```

- **`headers`**: 1행(row 0) 셀 텍스트. 빈 셀은 `col_<idx>`로 채움 (rows 객체 키 충돌 회피).
- **`rows`**: 2행 이후 셀의 `{header_key: cell_text}` 매핑. 멀티라인 셀은 `<br>` 결합 (D26).
- **`merged_cells`**: 병합 앵커 셀의 `(r, c, rowspan, colspan)`만 기록. 나머지 셀은 `rows`에서 빈 문자열 (D25).
- **`markdown`**: 임베딩·검색용. 셀의 `|`는 `\|`로 escape, 줄바꿈은 `<br>`로 치환.

---

## 5. `image` 상세

```jsonc
{
  "type": "image",
  "id": "b0020",
  "page": null,
  "heading_path": ["주문서", "2. 첨부"],
  "asset_ref": "img_3",
  "width_mm": 80.0,
  "height_mm": 60.0,
  "alt": null,
  "near_text_before": "다음 로고를 사용한다.",
  "near_text_after": "사용 시 색상을 유지한다."
}
```

- **`asset_ref`**: `assets[asset_ref]` 키. 동일 이미지 (`bin_data_id`) 재사용 시 같은 ref.
- **`width_mm` / `height_mm`**: HWP unit (1/7200 인치)에서 mm로 변환.
- **`alt`**: v1 항상 `null`. HWP 자체에 alt 슬롯이 없음. v2에서 caption 블록을 alt로 노출 검토.
- **`near_text_before/after`**: 직전·직후 비어있지 않은 paragraph/heading의 텍스트. 검색 회수 보강용 (D2).
- **`caption`**: 본 블록에는 없음. 별도 `type: caption` 블록으로 출력 (D29).

---

## 6. `assets`

```jsonc
{
  "img_3": {
    "mime": "image/png",
    "path": "order.assets/img_3.png",
    "sha256": "cd34...",
    "byte_size": 12345
  }
}
```

| 필드 | 의미 |
|------|------|
| `mime` | 확장자 기반 추정 (`png`, `jpeg`, `bmp`, `gif`, `svg+xml`, `wmf`, `emf`, `octet-stream`) |
| `path` | **D4 기본 (`extract` 모드)**. JSON 파일 기준 상대 경로 또는 `--image-dir` 절대 경로 |
| `data_base64` | `--image-mode inline` 시. base64 표준 인코딩 |
| `sha256` | 이미지 바이트 SHA-256 hex. dedup 키 |
| `byte_size` | 원본 바이트 길이 |

`path`와 `data_base64`는 정확히 한 쪽만 설정된다. 모드 전환은 CLI 옵션으로 결정.

---

## 7. SemVer 정책 (D13)

- **MAJOR** (`2.0.0`): 기존 필드 의미 변경, 필드 제거, 블록 type 제거 → consumer 거부 권장.
- **MINOR** (`1.1.0`): 신규 선택 필드 추가, 신규 block `type` 추가, 신규 asset 필드 추가 → forward-compatible.
- **PATCH** (`1.0.1`): 문서 수정, 변환 휴리스틱 미세 조정 (출력 형태 동일).

---

## 8. 일부러 빼는 항목

RAG 검색에 무용한 노이즈는 출력하지 않는다:

- 글꼴·색상·CharShape (`char_shapes`, `font_id`, `color`)
- 페이지 정의 (`width`, `height`, `margin_*`)
- `line_segs`, `para_shape_id`, `style_id`
- 빈 문단 (텍스트가 NFC 후 `""`인 paragraph)

향후 이미지 OCR·구조 보강 시에는 별도 `enrichment` 블록을 도입하지, 본 스키마를 오염시키지 않는다.
