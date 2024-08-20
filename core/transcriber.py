import os
import time
import psutil
import torch
import librosa
import numpy as np
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
        self._is_cancelled = False

    def run(self):
        total_files = len(self.files)
        for index, audio_file in enumerate(self.files, 1):
            if self._is_cancelled:
                break
            if ' > ' in audio_file:
                subfolder, filename = audio_file.split(' > ')
                input_path = os.path.join(self.directory, subfolder, filename)
                output_path = os.path.join(self.base_output_dir, subfolder, f"{os.path.splitext(filename)[0]}.txt")
            else:
                input_path = os.path.join(self.directory, audio_file)
                output_path = os.path.join(self.base_output_dir, f"{os.path.splitext(audio_file)[0]}.txt")
            
            self.transcribe_audio(input_path, output_path)
            self.progress_update.emit(index, total_files)
        
        if not self._is_cancelled:
            self.all_transcriptions_done.emit()


    def transcribe_audio(self, input_path, output_path):
        start_time = time.time()
        
        corrected_input_path = os.path.normpath(input_path)
        
        if not os.path.exists(corrected_input_path):
            raise FileNotFoundError(f"No se pudo encontrar el archivo: {corrected_input_path}")
        
        audio_duration = get_audio_duration(corrected_input_path)
        
        # Configurar parámetros de generación
        generate_kwargs = {
            "task": "translate" if self.translate else "transcribe",
            "language": self.language if not self.auto_detect else None,
            "max_new_tokens": 256,
            "temperature": self.transcription_options.get('temperature', 0.0),
            "do_sample": False,
            "num_beams": 1
        }

        if self.cpu_limit == "75%":
            p = psutil.Process()
            p.cpu_affinity([i for i in range(psutil.cpu_count()) if i % 4 != 3])

        result = self.pipe(corrected_input_path, return_timestamps=True, generate_kwargs=generate_kwargs)
        
        text = self.format_text(result["text"])
        
        output_dir = os.path.dirname(output_path)
        os.makedirs(output_dir, exist_ok=True)
        with open(output_path, 'w', encoding='utf-8') as f:
            f.write(text + "\n\n")  # Escribir el texto formateado
            f.write("Transcripción con timestamps:\n")
            if "chunks" in result:
                for chunk in result["chunks"]:
                    start = chunk.get('timestamp', [0, 0])[0]
                    end = chunk.get('timestamp', [0, 0])[1]
                    chunk_text = chunk.get('text', '')
                    f.write(f"[{self.format_timestamp(start)} -> {self.format_timestamp(end)}] {chunk_text}\n")
            else:
                import json
                f.write(json.dumps(result, indent=2))
        
        end_time = time.time()
        transcription_time = end_time - start_time
        self.transcription_done.emit(input_path, transcription_time, audio_duration)

    def format_timestamp(self, seconds):
        minutes, seconds = divmod(seconds, 60)
        return f"{int(minutes):02d}:{seconds:05.2f}" if minutes > 0 else f"{seconds:.2f}s"

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

    def cancel(self):
        self._is_cancelled = True
        if hasattr(self.pipe, 'model'):
            self.pipe.model.generation_config.max_new_tokens = 0