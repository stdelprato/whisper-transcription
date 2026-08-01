from PyQt6.QtCore import QThread, pyqtSignal
import subprocess
import re

class FasterWhisperXXLThread(QThread):
    output_received = pyqtSignal(str)
    progress_update = pyqtSignal(str, str, str)
    transcription_done = pyqtSignal(str, bool)

    def __init__(self, command, audio_file):
        super().__init__()
        self.command = command
        self.audio_file = audio_file

    def run(self):
        try:
            process = subprocess.Popen(self.command, shell=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1, universal_newlines=True)
            
            for line in iter(process.stdout.readline, ''):
                self.output_received.emit(line.strip())
                match = re.search(r'\[(\d{2}:\d{2}\.\d{3}) --> (\d{2}:\d{2}\.\d{3})\]', line)
                if match:
                    start, end = match.groups()
                    self.progress_update.emit(start, end, self.audio_file)
                elif line.startswith("["):  # Para capturar otros tipos de líneas de progreso
                    parts = line.split()
                    if len(parts) >= 2:
                        self.progress_update.emit(parts[0][1:], parts[0][1:], self.audio_file)
            
            process.wait()
            
            success = process.returncode == 0
            self.transcription_done.emit(self.audio_file, success)
        except Exception as e:
            self.output_received.emit(f"Error: {str(e)}")
            self.transcription_done.emit(self.audio_file, False)