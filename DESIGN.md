# DESIGN.md — rhwp-batch 워크스페이스 설계 진입점

> 본 문서는 두 멤버 crate의 설계 문서를 가리키는 **진입점**이다.
> 다른 문서: [README.md](README.md) · [ROADMAP.md](ROADMAP.md) · [CLAUDE.md](CLAUDE.md)

| crate | 설계 문서 | 다루는 것 |
|-------|----------|----------|
| `rhwp` (무수정, 분석 문서는 예외) | [rhwp/DESIGN.md](rhwp/DESIGN.md) | 입출력 파이프라인, IR 모델, `DocumentCore` 활용, 배치 모드 IR clone 전략, 라운드트립 보장 범위, 재사용 자산 매핑표 |
| `rhwp-batch` (신규) | [rhwp-batch/DESIGN.md](rhwp-batch/DESIGN.md) | 모듈 구조 (`cli`/`service`/`adapter`/`dto`/`template`/`batch`), 데이터 흐름, JMESPath 마커 평가, 오류 처리 전략 |
