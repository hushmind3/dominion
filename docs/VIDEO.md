# 영상 생성 (ComfyUI + Wan 2.1)

영상 메뉴는 ComfyUI를 영상 엔진으로 사용합니다. 채팅 모델과 별개이며, 워크플로마다 사용하는 영상 모델이 다릅니다.

## 1. ComfyUI 설치

```powershell
git clone --depth 1 https://github.com/comfyanonymous/ComfyUI C:\ComfyUI
uv venv --python 3.12 C:\ComfyUI\.venv
uv pip install --python C:\ComfyUI\.venv\Scripts\python.exe torch torchvision torchaudio --index-url https://download.pytorch.org/whl/cu126
uv pip install --python C:\ComfyUI\.venv\Scripts\python.exe -r C:\ComfyUI\requirements.txt
```

## 2. Wan 2.1 1.3B 모델

```powershell
$base='https://huggingface.co/Comfy-Org/Wan_2.1_ComfyUI_repackaged/resolve/main/split_files'
curl.exe -L --fail -o C:\ComfyUI\models\diffusion_models\wan2.1_t2v_1.3B_fp16.safetensors "$base/diffusion_models/wan2.1_t2v_1.3B_fp16.safetensors"
curl.exe -L --fail -o C:\ComfyUI\models\text_encoders\umt5_xxl_fp8_e4m3fn_scaled.safetensors "$base/text_encoders/umt5_xxl_fp8_e4m3fn_scaled.safetensors"
curl.exe -L --fail -o C:\ComfyUI\models\vae\wan_2.1_vae.safetensors "$base/vae/wan_2.1_vae.safetensors"
```

약 9.8GB입니다. 8GB VRAM에서는 `--lowvram`으로 실행하세요.

## 3. 워크플로

`examples/video-workflows/Wan 2.1 T2V 1.3B.json`을 아래 폴더에 복사합니다.

```
C:\Users\<사용자>\Documents\DOMINION Workspace\video-workflows\
```

워크플로는 ComfyUI의 **API 형식** JSON이며 아래 자리표시자를 지원합니다.

- `{{prompt}}`, `{{negative}}`
- `{{width}}`, `{{height}}`
- `{{frames}}`, `{{fps}}`, `{{seed}}`

## 4. 실행

1. 영상 제작 화면에서 **ComfyUI 시작**을 누릅니다. (또는 `C:\ComfyUI\.venv\Scripts\python.exe C:\ComfyUI\main.py --lowvram`)
2. 워크플로를 선택합니다.
3. 프롬프트와 화면 비율, 길이를 정합니다.
4. **영상 생성**을 누릅니다.
5. 완성된 결과를 미리 본 뒤 **업로드 폼에 채우기**로 YouTube 업로드로 넘깁니다.

2초 480×832 세로 영상은 RTX 3070에서 약 2~3분 걸립니다.
