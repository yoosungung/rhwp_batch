# ops_guide.md — rhwp-batch 운영 가이드

> **역할**: K8s/Airflow 환경에서 `rhwp-batch` 컨테이너를 운영하기 위한 가이드.
> 양식 마운트, 입출력 볼륨, 로그 수집, 자원 산정, 트러블슈팅을 다룬다.
> CLI 사양은 [README.md](../README.md), 마일스톤·결정은 [ROADMAP.md](../ROADMAP.md) 참조.

---

## 1. 컨테이너 이미지

- 빌드: [Dockerfile.batch](Dockerfile.batch) (multi-stage, debian-slim 런타임).
- 폰트: NanumGothic만 동봉 (D8, v1).
- 기본 ENTRYPOINT: `/usr/local/bin/rhwp-batch`.
- 권장 태그: `registry.local/rhwp-batch:0.1.0` (D9).

빌드 예시:
```bash
docker build -f deploy/Dockerfile.batch -t registry.local/rhwp-batch:0.1.0 .
docker push registry.local/rhwp-batch:0.1.0
```

---

## 2. 양식(template) 마운트 (D6, D22)

양식 HWP는 **컨테이너 이미지에 굽지 않고** 외부 경로에서 마운트한다.

### 2.1 ConfigMap (소형 양식, ≤ 1 MiB)

```bash
kubectl create configmap rhwp-templates \
  --from-file=order.hwp=./templates/order.hwp \
  --from-file=invoice.hwp=./templates/invoice.hwp
```

```yaml
volumes:
  - name: templates
    configMap:
      name: rhwp-templates
volumeMounts:
  - name: templates
    mountPath: /templates
    readOnly: true
```

ConfigMap은 etcd에 저장되므로 1 MiB 제한. 그림이 많은 양식은 PVC 사용.

### 2.2 PVC + S3 sync sidecar (대형 양식, ≥ 1 MiB 또는 다수)

```yaml
spec:
  initContainers:
    - name: template-sync
      image: amazon/aws-cli:2
      command:
        - sh
        - -c
        - aws s3 sync s3://my-bucket/templates/ /templates/
      volumeMounts:
        - { name: templates, mountPath: /templates }
  containers:
    - name: rhwp
      image: registry.local/rhwp-batch:0.1.0
      ...
  volumes:
    - name: templates
      emptyDir: {}
```

S3 직접 mount(CSI 드라이버)는 v2 옵션. v1은 init container로 sync.

### 2.3 양식 갱신 — 이미지 재빌드 불필요 (D11)

ConfigMap을 갱신하거나 S3 객체를 교체하면 다음 Job 실행부터 즉시 반영. 컨테이너 이미지 재빌드·재배포 없음.

---

## 3. 입출력 볼륨 패턴

### 3.1 fill 배치

```yaml
volumes:
  - name: templates
    configMap: { name: rhwp-templates }     # 양식 (read-only)
  - name: input
    persistentVolumeClaim: { claimName: rhwp-input }  # JSON 데이터 N건
  - name: output
    persistentVolumeClaim: { claimName: rhwp-output } # 채워진 HWP N건
```

데이터 디렉토리 구조 예:
```
/input/
  001.json
  002.json
  ...
/output/
  001.hwp
  002.hwp
  _report.json     # --report 출력
```

### 3.2 to-json 변환

```yaml
volumes:
  - name: input
    persistentVolumeClaim: { claimName: rhwp-input }   # HWP/HWPX
  - name: output
    persistentVolumeClaim: { claimName: rhwp-output }  # JSON + .assets/
```

`--image-mode extract` (D4 기본) 사용 시 출력 디렉토리에 `<basename>.assets/` 디렉토리가 함께 생성된다. 다운스트림이 path 참조를 풀 수 있도록 input/output PVC를 같은 마운트 트리에 두는 패턴 권장.

---

## 4. K8s Job 매니페스트

기본 매니페스트는 [job-template.yaml](k8s/job-template.yaml) 참조.

### 4.1 권장 설정

| 항목 | 값 | 비고 |
|------|----|------|
| `restartPolicy` | `OnFailure` | 일시적 IO 오류 재시도 |
| `backoffLimit` | `2` | 무한 재시도 방지. 부분 실패(exit 4)는 재시도 시 중복 출력 위험 → `--overwrite` 동반 검토 |
| `activeDeadlineSeconds` | 양식·N에 따라 산정 (예: 100건 × 양식 1MB → 600s) | 무한 행잉 방지 |
| `ttlSecondsAfterFinished` | `3600` | 완료된 Job 자동 정리 |

### 4.2 자원 요청 (배치 100건 기준 추정)

| 자원 | requests | limits | 근거 |
|------|---------|-------|------|
| cpu | 500m | 2 | 직렬 처리 시 1 코어로 충분, 병렬(`--threads N`) 시 N 코어 |
| memory | 512Mi | 2Gi | 양식 IR 1개 + N개 워커 clone (D15 기본 = CPU 수). 큰 양식은 워커 수 제한. |

큰 양식(수십 MB) 또는 이미지 다수 시 `limits.memory` 상향 필요. v1은 `bin_data_content`도 워커마다 clone되므로 메모리는 `(양식 IR + 이미지 합) × 워커 수`.

---

## 5. Airflow 통합

`KubernetesPodOperator` 사용 예시는 [airflow-dag-example.py](k8s/airflow-dag-example.py) 참조.

### 5.1 종료 코드 처리 (D10)

| exit | 의미 | Airflow 동작 |
|------|------|--------------|
| 0 | 전체 성공 | 태스크 성공 |
| 1 | 치명 실패 (양식 파싱 실패 등) | 태스크 실패 → 재시도 |
| 2 | CLI 인자 오류 | 태스크 실패 (재시도 무의미, 코드 수정 필요) |
| 3 | 출력 파일 충돌 (`--overwrite` 미지정) | 태스크 실패 → DAG에서 사전 정리 |
| 4 | **부분 실패** (`--on-error=continue` 사용 시) | 태스크 실패 → `_report.json` 파싱하여 후속 분기 |

`--on-error=continue` + `--report` 조합 권장: 한 건 실패가 전체를 막지 않으면서, 실패 항목은 다음 태스크에서 재처리·통보 가능.

### 5.2 `_report.json` 형식

```jsonc
{
  "total": 100,
  "succeeded": 98,
  "failed": 2,
  "items": [
    { "data": "/input/001.json", "output": "/output/001.hwp", "status": "ok" },
    { "data": "/input/042.json", "output": "/output/042.hwp", "status": "failed",
      "error": "Missing marker key 'customer.email'" }
  ]
}
```

후속 Airflow 태스크에서 `failed` 항목만 추려 알림(Slack/이메일) 발송하는 패턴 권장.

---

## 6. 로깅

### 6.1 로그 포맷

`--log-format json` 권장 (운영 환경 default). `tracing-subscriber`의 JSON 출력으로 stdout에 한 줄당 한 이벤트.

### 6.2 수집 패턴

- **표준 stdout**: Fluent Bit / Vector 등으로 수집.
- **span timing** (D20): `to_json_single`, `process_one` 등 `#[instrument]` 스팬에 처리 시간이 포함됨. 배치 콜드 스타트 비용 측정에 사용.
- **민감 정보**: 양식 내용·데이터 JSON은 로그에 직접 출력하지 않음 (file path와 sha256만). 그러나 에러 메시지에 일부 텍스트가 포함될 수 있음 → 운영 환경에서 PII가 의심되면 `--log-level error` 권장.

### 6.3 권장 필드 추출

| 필드 | 용도 |
|------|------|
| `target=rhwp_batch::service` | 단일 처리 |
| `target=rhwp_batch::batch` | 배치 처리 |
| `succeeded`, `failed`, `total` | 배치 결과 메트릭 |
| `sha256` | 입력 식별 |

---

## 7. 트러블슈팅

| 증상 | 원인 후보 | 조치 |
|------|----------|------|
| exit 1 + "HWP parse error" | 양식 파일 손상, HWP 5.0 미만 버전 | 양식 한컴오피스에서 다시 저장 |
| exit 1 + "Missing marker key" | 양식 마커와 데이터 키 불일치 | 데이터 JSON 검증, 또는 `--on-missing-key empty` |
| exit 3 + "output exists" | 이전 실행이 남긴 출력 | `--overwrite` 또는 사전 정리 init container |
| 출력 HWP가 한컴에서 글꼴 깨짐 | NanumGothic 외 폰트가 양식에 사용됨 | 사내 표준 폰트 동봉 이미지 별도 빌드 (v2) |
| 처리 시간이 양식 크기에 비례 폭증 | 매 항목마다 페이지네이션 트리거 (D18) | `tracing` 측정 후 임계 초과 시 v1.1에서 트랜잭션 API PR 검토 |
| 메모리 사용량 폭증 | `--threads`가 양식 크기 대비 과다 | `--threads`를 명시적으로 낮춤 (예: 큰 양식은 `--threads 1`) |
| 출력 디렉토리에 `.assets/` 폴더 누락 | `--image-mode inline` 사용 중 | path 참조가 필요하면 `--image-mode extract` (기본) 사용 |

---

## 8. 보안·격리

- **인증·권한** (D7): 본 도구는 미포함. K8s namespace + serviceaccount + NetworkPolicy로 격리.
- **파일 시스템 권한**: 컨테이너는 root로 실행. 이미지에서 non-root 유저로 변경하려면 빌드 시 `USER 65532` 추가.
- **Secret 처리**: 양식이 Secret을 포함하지 않도록 사전 점검. 민감 데이터가 출력 HWP에 들어간다면 PVC 접근 권한을 RBAC로 제한.

---

## 9. 향후 (v2 후보)

- HWPX 출력 (D5): `serialize_hwpx` 안정화 후 도입.
- S3 native 마운트 (CSI): init container 우회.
- 사내 표준 폰트 별도 이미지: NanumGothic 외 사내 표준 폰트 동봉.
- 워커 간 `bin_data_content` 공유 (Arc/COW): 큰 양식 메모리 폭증 완화.
