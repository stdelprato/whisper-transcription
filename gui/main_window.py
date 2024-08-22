import os
import json
import time
from utils.audio_utils import get_audio_duration, format_duration
from PyQt6.QtCore import Qt, QTimer
from PyQt6.QtWidgets import QMainWindow, QButtonGroup, QComboBox, QApplication, QWidget, QVBoxLayout, QHBoxLayout, QPushButton, QLabel, QLineEdit, QListWidget, QTextEdit, QFileDialog, QSlider, QProgressBar
from gui.ui_components import setup_ui, setup_connections, set_style
from core.file_loader import LoadFilesThread
from core.transcriber import TranscriptionThread
from core.whisper_model import load_whisper_model, load_faster_whisper_model, load_original_whisper_model
from utils.time_utils import format_time
import config

class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        
        self.base_dir = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
        self.json_path = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'transcription_data.json')
        
        self.setWindowTitle(config.APP_TITLE)
        self.setGeometry(100, 100, 800, 600)

        self.central_widget = QWidget()
        self.setCentralWidget(self.central_widget)
        self.layout = QVBoxLayout(self.central_widget)

        self.total_transcription_time = 0
        self.total_audio_duration = 0

        self.selected_model = "faster-whisper"
        self.pipe = None  # Inicializamos pipe a None
        self.audio_files = []
        self.current_language = None
        self.translate = False
        self.auto_detect = False
        self.load_thread = None

        self.transcription_data = self.load_transcription_data()

        setup_ui(self)
        self.setup_temperature_selection()
        self.auto_detect_btn.setChecked(True)
        self.on_lang_button_clicked(self.auto_detect_btn)
        self.update_transcribe_buttons()
        setup_connections(self)
        self.update_button_states()

        set_style(self)
        
        time_layout = QHBoxLayout()
        self.estimate_label = QLabel("Tiempo estimado: N/A")
        time_layout.addWidget(self.estimate_label)
        time_layout.addStretch()
        self.elapsed_time_label = QLabel("Tiempo transcurrido: 00:00")
        time_layout.addWidget(self.elapsed_time_label)
        self.layout.addLayout(time_layout)
        
        self.elapsed_timer = QTimer(self)
        self.elapsed_timer.timeout.connect(self.update_elapsed_time)
        self.elapsed_seconds = 0
        
        for button in self.temp_buttons.buttons():
            button.clicked.connect(self.update_estimate)
        
        # Conectar los botones de selección de modelo
        self.faster_whisper_btn.clicked.connect(lambda: self.set_model("faster-whisper"))
        self.original_whisper_btn.clicked.connect(lambda: self.set_model("original-whisper"))

        # Asegurarse de que el modelo correcto esté seleccionado y cargado por defecto
        self.faster_whisper_btn.setChecked(True)
        self.set_model("faster-whisper")

        self.transcription_data = self.load_transcription_data()

    def on_lang_button_clicked(self, button):
        self.auto_detect = False
        self.translate = False
        self.current_language = None

        if button == self.auto_detect_btn:
            self.auto_detect = True
            self.translate = True
            print("Auto-detect and translate selected")
        elif button == self.es_btn:
            self.current_language = 'es'
            # No cambiamos el estado de traducción aquí, se maneja con set_translation
        elif button == self.en_btn:
            self.current_language = 'en'
        else:  # Auto-detectar (sin traducción)
            self.auto_detect = True
            print("Auto-detect (without translation) selected")

        self.translate_btn.setEnabled(button == self.es_btn)
        self.no_translate_btn.setEnabled(button == self.es_btn)

        if button == self.es_btn:
            if not self.translate_btn.isChecked() and not self.no_translate_btn.isChecked():
                self.translate_btn.setChecked(True)
                self.translate = True

        self.update_button_states()
        print(f"auto_detect: {self.auto_detect}, translate: {self.translate}, language: {self.current_language}")

    def estimate_transcription_time(self, audio_duration):
        if self.total_audio_duration == 0:
            return "Unknown"
        
        avg_speed = self.total_transcription_time / self.total_audio_duration
        estimated_time = audio_duration * avg_speed
        return format_time(estimated_time)

    def get_temperature_from_quality(self):
        for button in self.temp_buttons.buttons():
            if button.isChecked():
                quality = button.text()
                if quality == "Muy bueno":
                    return 0.000001
                elif quality == "Bueno":
                    return 0.1
                elif quality == "Regular":
                    return 0.2
                else:  # Mala
                    return 0.3
        return 0.000001  # Default to "Muy bueno" if nothing is selected

    def setup_temperature_selection(self):
        self.temp_layout = QHBoxLayout()
        self.temp_label = QLabel("Calidad del audio:")
        self.temp_layout.addWidget(self.temp_label)
        
        self.temp_buttons = QButtonGroup(self)
        qualities = ["Muy bueno", "Bueno", "Regular", "Mala"]
        for quality in qualities:
            btn = QPushButton(quality)
            btn.setCheckable(True)
            self.temp_buttons.addButton(btn)
            self.temp_layout.addWidget(btn)
        
        self.temp_buttons.buttonClicked.connect(self.update_temperature)
        self.temp_buttons.buttons()[0].setChecked(True)  # Set "Muy bueno" as default
        
        # Insertar este layout antes de los botones de transcripción
        insert_index = self.layout.indexOf(self.trans_btn_layout)
        self.layout.insertLayout(insert_index, self.temp_layout)

    def update_temperature(self, button):
        for btn in self.temp_buttons.buttons():
            if btn != button:
                btn.setChecked(False)

    def set_model(self, model):
        self.selected_model = model
        self.pipe = None  # Reseteamos el pipe para que se cargue el nuevo modelo cuando sea necesario
        print(f"Modelo seleccionado: {model}")

    def update_elapsed_time(self):
        self.elapsed_seconds += 1
        self.elapsed_time_label.setText(f"Tiempo transcurrido: {format_duration(self.elapsed_seconds)}")
        self.update_progress()  # Llamamos a update_progress aquí para actualizar la barra de progreso

    def update_progress(self):
        if not hasattr(self, 'transcription_start_time'):
            return

        elapsed_time = time.time() - self.transcription_start_time
        estimated_time_text = self.estimate_label.text()
        
        if "N/A" not in estimated_time_text:
            estimated_time = self.parse_estimated_time(estimated_time_text)
            if estimated_time > 0:
                percentage = min(100, (elapsed_time / estimated_time) * 100)
                self.progress_bar.setValue(int(percentage))
                self.progress_bar.setFormat(f"{percentage:.2f}%")
            else:
                self.progress_bar.setFormat("Progreso: N/A")
        else:
            self.progress_bar.setFormat("Progreso: N/A")

    def parse_estimated_time(self, time_text):
        time_parts = time_text.split(": ")[1].split(" ")[0].split(":")
        hours, minutes, seconds = map(int, time_parts)
        return hours * 3600 + minutes * 60 + seconds

    def browse_folder(self):
        folder = QFileDialog.getExistingDirectory(self, "Seleccionar carpeta de audio")
        if folder:
            self.folder_input.setText(folder)
            QApplication.processEvents()
            self.load_files(folder)
    
    def load_files(self, directory):
        self.file_list.clear()
        self.audio_files = []
        
        if self.load_thread and self.load_thread.isRunning():
            self.load_thread.requestInterruption()
            self.load_thread.wait()
        
        self.load_thread = LoadFilesThread(directory)
        self.load_thread.file_found.connect(self.add_file_to_list)
        self.load_thread.finished.connect(self.on_file_loading_finished)
        
        self.setEnabled(False)
        
        self.load_thread.start()

    def closeEvent(self, event):
        if self.load_thread and self.load_thread.isRunning():
            self.load_thread.requestInterruption()
            self.load_thread.wait()
        event.accept()

    def add_file_to_list(self, file_path, relative_path):
        duration = get_audio_duration(file_path)
        duration_str = format_duration(duration)
        
        if relative_path == '.':
            item_text = f"({duration_str}) | {os.path.basename(file_path)}"
        else:
            item_text = f"({duration_str}) | {relative_path} > {os.path.basename(file_path)}"
        
        self.file_list.addItem(item_text)
        self.audio_files.append((file_path, duration))

    def on_file_loading_finished(self):
        self.setEnabled(True)
        self.update_transcribe_buttons()

    def set_language(self, lang):
        self.current_language = lang
        self.es_btn.setChecked(lang == 'es')
        self.en_btn.setChecked(lang == 'en')
        self.auto_btn.setChecked(lang is None)
        
        if lang == 'es':
            self.set_translation(True)
        else:
            self.set_translation(False)
        
        self.auto_detect_btn.setChecked(False)
        self.update_button_states()

    def set_translation(self, translate):
        self.translate = translate
        self.translate_to_english = translate
        self.translate_btn.setChecked(translate)
        self.no_translate_btn.setChecked(not translate)
        self.update_button_states()

    def update_button_states(self):
        is_spanish = self.current_language == 'es'
        self.translate_btn.setEnabled(is_spanish)
        self.no_translate_btn.setEnabled(is_spanish)
        if is_spanish:
            if not self.translate_btn.isChecked() and not self.no_translate_btn.isChecked():
                self.translate_btn.setChecked(True)
                self.no_translate_btn.setChecked(False)
                self.translate_to_english = True
        else:
            self.translate_btn.setChecked(False)
            self.no_translate_btn.setChecked(True)
            self.translate_to_english = False
    
    def get_transcription_options(self):
        for button in self.temp_buttons.buttons():
            if button.isChecked():
                quality = button.text()
                if quality == "Muy bueno":
                    return {
                        "quality": "Muy bueno",
                        "temperature": 0.000001,
                        "compression_ratio_threshold": 2.8,
                        "logprob_threshold": -1.5,
                        "no_speech_threshold": 0.6,
                        "condition_on_previous_text": True
                    }
                elif quality == "Bueno":
                    return {
                        "quality": "Bueno",
                        "temperature": 0.1,
                        "compression_ratio_threshold": 2.8,
                        "logprob_threshold": -1.6,
                        "no_speech_threshold": 0.55,
                        "condition_on_previous_text": True
                    }
                elif quality == "Regular":
                    return {
                        "quality": "Regular",
                        "temperature": 0.2,
                        "compression_ratio_threshold": 2.8,
                        "logprob_threshold": -1.7,
                        "no_speech_threshold": 0.5,
                        "condition_on_previous_text": True
                    }
                else:  # Mala
                    return {
                        "quality": "Mala",
                        "temperature": 0.3,
                        "compression_ratio_threshold": 2.8,
                        "logprob_threshold": -1.8,
                        "no_speech_threshold": 0.45,
                        "condition_on_previous_text": True
                    }
        return {  # Default to "Muy bueno" if nothing is selected
            "quality": "Muy bueno",
            "temperature": 0.000001,
            "compression_ratio_threshold": 2.8,
            "logprob_threshold": -1.5,
            "no_speech_threshold": 0.6,
            "condition_on_previous_text": True
        }
    
    def update_transcribe_buttons(self):
        selected_items = self.file_list.selectedItems()
        self.transcribe_selected_btn.setVisible(len(selected_items) > 0)
        
        self.transcribe_all_btn.setStyleSheet(
            "background-color: #2d8659; color: white;"
        )
        
        if len(selected_items) > 0:
            self.transcribe_selected_btn.setStyleSheet(
                "background-color: #1a5f7a; color: white;"
            )
            self.trans_btn_layout.setStretch(0, 80)
            self.trans_btn_layout.setStretch(1, 20)
        else:
            self.trans_btn_layout.setStretch(0, 0)
            self.trans_btn_layout.setStretch(1, 100)

    def transcribe(self, selected=True):
        if not self.audio_files:
            self.output_text.append("No hay archivos de audio cargados.")
            return

        if not self.pipe:
            self.output_text.append(f"Cargando modelo {self.selected_model}...")
            self.load_whisper_model()

        files_to_transcribe = [item.text().split(' | ', 1)[1] for item in self.file_list.selectedItems()] if selected else [self.file_list.item(i).text().split(' | ', 1)[1] for i in range(self.file_list.count())]
        
        if not files_to_transcribe:
            self.output_text.append("No se han seleccionado archivos para transcribir.")
            return

        self.output_text.append("Iniciando transcripción...")
        transcription_options = self.get_transcription_options()
        base_output_dir = os.path.join(self.base_dir, "transcription_results")

        self.progress_bar.setValue(0)
        self.progress_bar.setVisible(True)
        
        self.elapsed_seconds = 0
        self.elapsed_timer.start(1000)

        self.transcription_thread = TranscriptionThread(
            self.pipe, files_to_transcribe, self.folder_input.text(), 
            self.current_language, self.translate, 
            transcription_options, self.auto_detect, 
            base_output_dir, self.selected_model
        )
        self.transcription_thread.transcription_done.connect(self.on_transcription_done)
        self.transcription_thread.all_transcriptions_done.connect(self.on_all_transcriptions_done)
        self.transcription_thread.progress_update.connect(self.update_progress)

        self.transcription_start_time = time.time()
        self.progress_timer = QTimer(self)
        self.progress_timer.timeout.connect(self.update_progress)
        self.progress_timer.start(1000)

        self.transcription_thread.start()

    def load_whisper_model(self):
        if self.selected_model == "faster-whisper":
            self.pipe = load_faster_whisper_model()
        else:
            self.pipe = load_original_whisper_model()

    def on_transcription_done(self, file_path, transcription_time, audio_duration):
        transcription_options = self.get_transcription_options()
        
        self.transcription_data[file_path] = {
            'duration': audio_duration,
            'transcription_time': transcription_time,
            'quality': transcription_options['quality']
        }
        self.save_transcription_data()
        self.update_estimate()
        
        self.output_text.append(f"Transcripción completada: {file_path}")
        self.output_text.append(f"Tiempo de transcripción: {format_duration(transcription_time)}")

    def load_transcription_data(self):
        try:
            with open(self.json_path, 'r') as f:
                return json.load(f)
        except FileNotFoundError:
            return {}
        
    def save_transcription_data(self):
        with open(self.json_path, 'w') as f:
            json.dump(self.transcription_data, f, indent=4)
            
    def update_estimate(self):
        selected_items = self.file_list.selectedItems()
        if not selected_items:
            self.estimate_label.setText("Tiempo estimado: N/A")
            return
        
        current_quality = self.get_transcription_options()['quality']
        
        matching_transcriptions = [
            data for data in self.transcription_data.values()
            if data['quality'] == current_quality
        ]
        
        total_duration = sum(float(item.text().split('(')[1].split(')')[0].split(':')[0]) * 3600 +
                            float(item.text().split('(')[1].split(')')[0].split(':')[1]) * 60 +
                            float(item.text().split('(')[1].split(')')[0].split(':')[2])
                            for item in selected_items)
        
        if len(matching_transcriptions) < 3:
            # Si no hay suficientes datos, usa un promedio general
            all_transcriptions = list(self.transcription_data.values())
            if all_transcriptions:
                avg_speed = sum(data['transcription_time'] / data['duration'] for data in all_transcriptions) / len(all_transcriptions)
            else:
                avg_speed = 1  # Fallback si no hay datos en absoluto
            estimated_time = total_duration * avg_speed
            self.estimate_label.setText(f"Tiempo estimado: {format_duration(estimated_time)} (Estimación general)")
        else:
            avg_speed = sum(data['transcription_time'] / data['duration'] for data in matching_transcriptions) / len(matching_transcriptions)
            estimated_time = total_duration * avg_speed
            self.estimate_label.setText(f"Tiempo estimado: {format_duration(estimated_time)}")

    def on_all_transcriptions_done(self):
        self.output_text.append("COMPLETADO.")
        self.progress_bar.setVisible(False)
        self.elapsed_timer.stop()
        if hasattr(self, 'progress_timer'):
            self.progress_timer.stop()

    def clear_output(self):
        self.output_text.clear()
        self.progress_bar.setValue(0)
        self.progress_bar.setVisible(False)
        self.elapsed_seconds = 0
        self.elapsed_time_label.setText("Tiempo transcurrido: 00:00")