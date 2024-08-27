import os
import subprocess
import re
from PyQt6.QtCore import QThread, pyqtSignal

class SequentialTranscriptionThread(QThread):
    transcription_started = pyqtSignal(str)
    transcription_finished = pyqtSignal(str, bool)
    output_received = pyqtSignal(str)
    progress_update = pyqtSignal(str, str, str)

    def __init__(self, files_to_transcribe, executable_path, base_output_dir, folder_input, current_language, translate):
        super().__init__()
        self.files_to_transcribe = files_to_transcribe
        self.executable_path = executable_path
        self.base_output_dir = base_output_dir
        self.folder_input = folder_input
        self.current_language = current_language
        self.translate = translate

    def run(self):
        for audio_file in self.files_to_transcribe:
            self.transcription_started.emit(audio_file)
            if ' > ' in audio_file:
                subfolder, filename = audio_file.split(' > ')
                input_path = os.path.join(self.folder_input, subfolder, filename)
                output_subfolder = os.path.join(self.base_output_dir, subfolder)
            else:
                input_path = os.path.join(self.folder_input, audio_file)
                output_subfolder = self.base_output_dir

            os.makedirs(output_subfolder, exist_ok=True)
            
            language_option = f"-l {self.current_language}" if self.current_language else ""
            task_option = "translate" if self.translate else "transcribe"
            
            command = f'"{self.executable_path}" "{input_path}" {language_option} -m large-v2 --temperature 0.00001 --compression_ratio_threshold 2 --task {task_option} --sentence --output_dir "{output_subfolder}" --output_format txt'
            
            process = subprocess.Popen(command, shell=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1, universal_newlines=True)
            
            for line in iter(process.stdout.readline, ''):
                self.output_received.emit(line.strip())
                match = re.search(r'\[(\d{2}:\d{2}\.\d{3}) --> (\d{2}:\d{2}\.\d{3})\]', line)
                if match:
                    start, end = match.groups()
                    self.progress_update.emit(start, end, audio_file)
            
            process.wait()
            success = process.returncode == 0
            self.transcription_finished.emit(audio_file, success)