import torch
from transformers import AutoModelForSpeechSeq2Seq, AutoProcessor, pipeline

# Configurar el dispositivo (GPU si está disponible, de lo contrario CPU)
device = "cuda:0" if torch.cuda.is_available() else "cpu"
torch_dtype = torch.float16 if torch.cuda.is_available() else torch.float32

# Cargar el modelo y el procesador
model_id = "openai/whisper-large-v3"
model = AutoModelForSpeechSeq2Seq.from_pretrained(
    model_id, torch_dtype=torch_dtype, low_cpu_mem_usage=True, use_safetensors=True
)
model.to(device)

processor = AutoProcessor.from_pretrained(model_id)

# Crear el pipeline
pipe = pipeline(
    "automatic-speech-recognition",
    model=model,
    tokenizer=processor.tokenizer,
    feature_extractor=processor.feature_extractor,
    max_new_tokens=128,
    chunk_length_s=30,
    batch_size=16,
    return_timestamps=False,
    torch_dtype=torch_dtype,
    device=device,
)

# Ruta del archivo de audio (reemplaza esto con la ruta de tu archivo)
audio_path = "C:/Users/stdel/Documents/Sound Recordings/audio_dross.mp3"

# Configurar parámetros de generación
generate_kwargs = {
    "task": "translate",
    "language": None,  # Esto permitirá la detección automática del idioma
    "max_new_tokens": 256,
    "temperature": 0.0,
    "do_sample": False,
    "num_beams": 1,
}

# Realizar la transcripción y traducción
result = pipe(audio_path, generate_kwargs=generate_kwargs)

# Imprimir el resultado
print("Resultado de la traducción:")
print(result["text"])