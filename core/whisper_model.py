import torch
from transformers import AutoModelForSpeechSeq2Seq, AutoProcessor, pipeline
from faster_whisper import WhisperModel
import config

def load_original_whisper_model():
    device = "cuda:0" if torch.cuda.is_available() else "cpu"
    torch_dtype = torch.float16 if torch.cuda.is_available() else torch.float32

    model = AutoModelForSpeechSeq2Seq.from_pretrained(
        config.MODEL_ID, torch_dtype=torch_dtype, low_cpu_mem_usage=True, use_safetensors=True
    )
    model.to(device)

    processor = AutoProcessor.from_pretrained(config.MODEL_ID)

    pipe = pipeline(
        "automatic-speech-recognition",
        model=model,
        tokenizer=processor.tokenizer,
        feature_extractor=processor.feature_extractor,
        max_new_tokens=96,
        chunk_length_s=30,
        batch_size=16,
        return_timestamps=False,
        torch_dtype=torch_dtype,
        device=device,
    )

    import warnings
    warnings.filterwarnings("ignore", category=FutureWarning)
    warnings.filterwarnings("ignore", category=UserWarning)

    return pipe

def load_faster_whisper_model():
    device = "cuda:0" if torch.cuda.is_available() else "cpu"
    compute_type = "float16" if torch.cuda.is_available() else "int8"

    model_size = "large-v2"
    
    print("Cargando Faster-Whisper...")
    model = WhisperModel(
        model_size, 
        device=device, 
        compute_type=compute_type, 
        download_root=config.MODEL_DIR,
    )

    return model

def load_whisper_model(model_type="original"):
    print("Cargando Whisper Original...")
    if model_type == "original":
        return load_original_whisper_model()
    elif model_type == "faster":
        return load_faster_whisper_model()
    else:
        raise ValueError("Tipo de modelo no válido. Use 'original' o 'faster'.")
