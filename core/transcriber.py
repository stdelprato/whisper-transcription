import os
import time
import psutil
import torch
from transformers      import pipeline
from PyQt6.QtCore      import QThread, pyqtSignal
from utils.audio_utils import get_audio_duration, format_duration

class TranscriptionThread(QThread):
    transcription_done = pyqtSignal(str, float, float)
    all_transcriptions_done = pyqtSignal()
    progress_update = pyqtSignal(int, int)

    def __init__(self, pipe, files, directory, language, translate, transcription_options, auto_detect, base_output_dir, cpu_limit):
        super().__init__()
        self.pipe = pipe
        self.files = files
        self.directory = directory
        self.language = language
        self.translate = translate
        self.transcription_options = transcription_options
        self.auto_detect = auto_detect
        self.base_output_dir = base_output_dir
        self.cpu_limit = cpu_limit

    def run(self):
        total_files = len(self.files)
        for index, audio_file in enumerate(self.files, 1):
            if ' > ' in audio_file:
                subfolder, filename = audio_file.split(' > ')
                input_path = os.path.join(self.directory, subfolder, filename)
                output_path = os.path.join(self.base_output_dir, subfolder, f"{os.path.splitext(filename)[0]}.txt")
            else:
                input_path = os.path.join(self.directory, audio_file)
                output_path = os.path.join(self.base_output_dir, f"{os.path.splitext(audio_file)[0]}.txt")
            
            self.transcribe_audio(input_path, output_path)
            self.progress_update.emit(index, total_files)
        self.all_transcriptions_done.emit()

    def transcribe_audio(self, input_path, output_path):
        start_time = time.time()
        
        corrected_input_path = os.path.normpath(input_path)
        
        if not os.path.exists(corrected_input_path):
            raise FileNotFoundError(f"No se pudo encontrar el archivo: {corrected_input_path}")
        
        audio_duration = get_audio_duration(corrected_input_path)
        
        # Determinar la tarea y el idioma
        task = "translate" if self.auto_detect else ("translate" if self.translate and self.language == 'es' else "transcribe")
        language = self.language if not self.auto_detect else None
        
        # Extraer solo los parámetros que el pipeline puede manejar directamente
        generate_kwargs = {
            "chunk_length_s": 30,
            "batch_size": 8,
        }

        if self.cpu_limit == "75%":
            p = psutil.Process()
            p.cpu_affinity([i for i in range(psutil.cpu_count()) if i % 4 != 3])

        # Usar fp16 si está disponible
        if torch.cuda.is_available():
            generate_kwargs["fp16"] = True

        # Configurar la tarea y el idioma en el pipeline
        self.pipe.model.config.forced_decoder_ids = self.pipe.tokenizer.get_decoder_prompt_ids(task=task, language=language)

        # Configurar los parámetros de generación del modelo
        self.pipe.model.config.temperature = self.transcription_options.get('temperature', 0.0)
        self.pipe.model.config.compression_ratio_threshold = self.transcription_options.get('compression_ratio_threshold', 2.4)
        self.pipe.model.config.logprob_threshold = self.transcription_options.get('logprob_threshold', -1.0)
        self.pipe.model.config.no_speech_threshold = self.transcription_options.get('no_speech_threshold', 0.6)

        result = self.pipe(corrected_input_path, **generate_kwargs)
        
        text = self.format_text(result["text"])
        
        output_dir = os.path.dirname(output_path)
        os.makedirs(output_dir, exist_ok=True)
        with open(output_path, 'w', encoding='utf-8') as f:
            f.write(text)
        
        end_time = time.time()
        transcription_time = end_time - start_time
        self.transcription_done.emit(input_path, transcription_time, audio_duration)

    def format_text(self, text):
        import re
        if not text.endswith('.'):
            text += '.'
        sentences = re.split('(?<=[.!?]) +', text)
        sentences = [s.capitalize() for s in sentences]
        paragraphs = []
        for i in range(0, len(sentences), 4):
            paragraph = ' '.join(sentences[i:i+4])
            paragraphs.append(paragraph)
        formatted_text = '\n\n'.join(paragraphs)
        return formatted_text