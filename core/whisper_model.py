from faster_whisper import WhisperModel
import torch
import config

def load_whisper_model():
    device = "cuda:0" if torch.cuda.is_available() else "cpu"
    compute_type = "float16" if torch.cuda.is_available() else "int8"

    model_size = "large-v3"

    print(f"Cargando el modelo Faster-Whisper {model_size}...")
    model = WhisperModel(model_size, device=device, compute_type=compute_type, download_root=config.MODEL_DIR)

    return model