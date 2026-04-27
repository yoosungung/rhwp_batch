# CLAUDE.md

본 파일은 클로드 코드(claude.ai/code) 및 기여자가 본 저장소에서 작업할 때 따라야 할
**작업 계약·주의사항**을 정의한다. 빠른 실행·자율 수정 같은 클로드 코드 기본 동작과
충돌하므로 반드시 숙지한다.

> **다른 문서**:
> - 사용자 가이드(완성 후): [README.md](README.md)
> - 기술적 구조·정보: [DESIGN.md](DESIGN.md)
> - 향후 계획·결정 항목·위험 관리: [ROADMAP.md](ROADMAP.md)

---

## 프로젝트 한 줄 요약

`rhwp-batch` — Linux CLI 도구. 데이터 처리 파이프라인에서 HWP/HWPX → JSON 변환 및
양식 + JSON → 채워진 HWP 생성을 수행한다. 자세한 설명·사용법은
[README.md](README.md), 기술 근거는 [DESIGN.md](DESIGN.md), 진척은
[ROADMAP.md](ROADMAP.md) 참조.

현재 상태: **M1 착수 가능** — 워크스페이스/스켈레톤 완료, 결정 항목 D1~D29
(RAG-friendly DTO 포함) 모두 확정, git 원격 셋업 완료(`github.com/yoosungung/rhwp_batch`).
다음 단계는 M1 GitHub Issue 등록 → 브랜치 생성 → 수행 계획서 작성. 상세는
[ROADMAP.md](ROADMAP.md) 참조.

---

## 핵심 작업 규칙 (하이퍼-워터폴)

이 프로젝트는 상위 `rhwp` 프로젝트와 동일한 **하이퍼-워터폴** 방법론을 적용한다.

- **소스 수정 전 반드시 작업지시자 승인 요청**
- **워크스페이스 멤버 `rhwp` crate는 무수정 원칙** — `rhwp-batch`에서 use만 한다.
  필요 시 별도 이슈로 상위 저장소에 PR.
  - **예외**: 본 저장소 작업을 위한 분석 문서(예: `rhwp/DESIGN.md`)는 무수정 원칙
    적용 대상에서 제외한다. 코드(`rhwp/src/**`, `rhwp/Cargo.toml`)와 빌드 산출물은
    원칙을 그대로 따른다.
- **이슈 → 브랜치 → 할일 → 수행계획서 → 구현계획서 → 단계별 구현 → 단계별 보고 → 최종 보고**
  순서 절대 생략 금지
- 각 단계 완료 후 승인 없이 다음 단계 진행 금지
- 이슈 클로즈는 작업지시자 승인 후에만 수행
- 작업 시간의 시작과 종료는 작업지시자가 결정 — 클로드가 임의로 작업 종료를 제안하지 않는다

---

## 워크스페이스 구조

본 저장소는 Cargo 워크스페이스다. `rhwp`(원본, 무수정)와 `rhwp-batch`(신규)를 멤버로 둔다.

```
rhwp_batch/                       # 워크스페이스 루트 (본 저장소)
├── README.md                     # 사용자 가이드
├── CLAUDE.md                     # 본 문서 (클로드/기여자 작업 계약)
├── DESIGN.md                     # 기술 분석
├── ROADMAP.md                    # 마일스톤·결정·위험
├── Cargo.toml                    # [workspace] members = ["rhwp", "rhwp-batch"]
├── rhwp/                         # ref_repo에서 복사한 원본 crate (무수정 유지)
│   ├── Cargo.toml
│   └── src/                      # parser / serializer / document_core / model ...
├── rhwp-batch/                   # ★ 신규 crate
│   ├── Cargo.toml                # rhwp = { path = "../rhwp" }
│   ├── src/
│   │   ├── main.rs               # clap 진입점
│   │   ├── lib.rs
│   │   ├── cli/                  # to-json, fill 서브커맨드
│   │   ├── service/              # 변환·생성 로직
│   │   ├── dto/                  # JSON 스키마 (serde)
│   │   ├── adapter/              # IR ↔ DTO
│   │   ├── template/             # 마커 파싱·치환
│   │   ├── batch.rs              # 배치 러너
│   │   └── error.rs
│   ├── tests/                    # 통합 테스트
│   └── samples/templates/        # 양식 HWP 예시
└── deploy/
    ├── Dockerfile.batch
    └── k8s/
        ├── job-template.yaml
        └── airflow-dag-example.py
```

> 작업 중인 심볼릭 링크 `ref_repo → ../rhwp`는 내용을 본 저장소 `rhwp/`로 복사한 후 제거한다.

---

## 빌드 및 실행 (예정)

```bash
cargo build --release -p rhwp-batch    # rhwp-batch 릴리즈 빌드 (rhwp는 의존으로 자동 빌드)
cargo test -p rhwp-batch               # rhwp-batch 테스트
cargo test -p rhwp-batch <test_name>   # 단일 테스트
cargo test --workspace                 # 전 멤버 테스트 (rhwp 포함)
```

워크스페이스 멤버 `rhwp`는 무수정 원칙이므로 일반적으로 직접 수정·빌드하지 않는다.
Docker는 **컨테이너 이미지 산출용**으로만 사용한다. 사용자 사용법은 [README.md](README.md) 참조.

---

## 다른 문서의 역할

본 CLAUDE.md는 **클로드 코드/기여자의 작업 계약·절차** 만 다룬다. 작업 중 어떤
문서를 봐야 하는지·어떤 정보가 어디 정본인지는 아래 표가 정본이다.

| 문서 | 대상 | 역할 | 정본 영역 |
|------|------|------|----------|
| [README.md](README.md) | 사용자 (데이터 엔지니어·운영자) | 빠른 시작, CLI 사양, 양식 작성 가이드, 운영 예시 | CLI 사양·옵션·종료 코드·비-목표 |
| [DESIGN.md](DESIGN.md) | 개발자·리뷰어 | 진입점 (두 하위 DESIGN.md로 라우팅) | — |
| [rhwp/DESIGN.md](rhwp/DESIGN.md) | 개발자·리뷰어 | rhwp 라이브러리 분석, IR 모델, DocumentCore 활용, 자산 매핑표 | rhwp API 부수 효과·메모리·동시성 |
| [rhwp-batch/DESIGN.md](rhwp-batch/DESIGN.md) | 개발자·리뷰어 | rhwp-batch crate 모듈 구조·데이터 흐름 | rhwp-batch 내부 설계 |
| [ROADMAP.md](ROADMAP.md) | 계획자·기여자 | 결정 항목, 마일스톤, 위험 관리 | 결정 항목 표·마일스톤·위험 표 |

> 정본 영역이 어긋나면 표시된 정본 문서를 우선한다. CLAUDE.md는 위 문서의
> 내용을 중복으로 보유하지 않는다.

