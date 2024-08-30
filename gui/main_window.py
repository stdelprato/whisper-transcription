import os
import re
import subprocess
import sys
import threading
import json
import time
from core.sequential_transcription_thread import SequentialTranscriptionThread
from core.faster_whisper_thread import FasterWhisperXXLThread
from utils.audio_utils import get_audio_duration, format_duration
from PyQt6.QtCore import Qt, QSize, QTimer, QThread, pyqtSignal, QDir, QFileSystemWatcher
from PyQt6.QtGui import QIcon, QFont, QColor, QIcon, QStandardItemModel, QStandardItem
from PyQt6.QtWidgets import QHBoxLayout, QLabel, QMessageBox, QSplitter, QTreeView, QMainWindow, QButtonGroup, QComboBox, QApplication, QWidget, QVBoxLayout, QHBoxLayout, QPushButton, QLineEdit, QListWidget, QTextEdit, QFileDialog, QSlider, QProgressBar
from gui.ui_components import setup_ui, setup_connections, set_style
from core.file_loader import LoadFilesThread
from core.transcriber import TranscriptionThread
from core.whisper_model import load_whisper_model, load_faster_whisper_model, load_original_whisper_model
from utils.time_utils import format_time
from timestamp_notepad import TimestampNotepad
import shutil
import config

class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        
        self.base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        self.json_path = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'transcription_data.json')
        
        self.file_watcher = QFileSystemWatcher()
        self.file_watcher.directoryChanged.connect(self.on_directory_changed)
        
        # Agregar el directorio de transcripciones al watcher
        transcription_dir = os.path.join(self.base_dir, "transcription_results")
        if os.path.exists(transcription_dir):
            self.file_watcher.addPath(transcription_dir)

        self.setWindowTitle(config.APP_TITLE)
        self.setGeometry(100, 100, 800, 600)

        self.central_widget = QWidget()
        self.setCentralWidget(self.central_widget)
        self.layout = QVBoxLayout(self.central_widget)

        self.total_transcription_time = 0
        self.total_audio_duration = 0

        self.selected_model = "faster-whisper-xxl"
        self.pipe = None
        self.audio_files = []
        self.current_language = 'es'  # Establecer español como idioma predeterminado
        self.translate = True  # Establecer traducción como opción predeterminada
        self.auto_detect = False
        self.load_thread = None

        self.transcription_data = self.load_transcription_data()

        setup_ui(self)
        self.setup_temperature_selection()
        self.setup_sidebar()

        self.has_faster_whisper_xxl = self.check_faster_whisper_xxl_folder()
        
        self.setup_faster_whisper_xxl_button()
        
        setup_connections(self)
        
        self.es_btn.setChecked(True)
        self.translate_btn.setChecked(True)
        self.update_button_states()
        self.update_transcribe_buttons()

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

        # Establecer el modelo predeterminado
        self.set_model("faster-whisper-xxl")

        self.transcription_data = self.load_transcription_data()

    def on_lang_button_clicked(self, button):
        self.auto_detect = False
        self.current_language = None

        if button == self.auto_btn:
            self.auto_detect = True
            self.translate = False
        elif button == self.es_btn:
            self.current_language = 'es'
            self.translate = True  # Establecer traducción como predeterminada para español
        elif button == self.en_btn:
            self.current_language = 'en'
            self.translate = False

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
        self.pipe = None
        
        # Actualizar el estado de los botones y su estilo
        buttons = [
            (self.faster_whisper_xxl_btn, "faster-whisper-xxl"),
            (self.faster_whisper_btn, "faster-whisper"),
            (self.original_whisper_btn, "original-whisper")
        ]
        
        for button, button_model in buttons:
            is_selected = (model == button_model)
            button.setChecked(is_selected)
            if is_selected:
                button.setStyleSheet("background-color: #3A7CA5; color: white;")
            else:
                button.setStyleSheet("")  # Resetear al estilo por defecto
        
        # Habilitar/deshabilitar botones de calidad según el modelo seleccionado
        for button in self.temp_buttons.buttons():
            button.setEnabled(model != "faster-whisper-xxl")
        
        print(f"Modelo seleccionado: {model}")

    def check_faster_whisper_xxl_folder(self):
        for item in os.listdir(self.base_dir):
            if item.lower() == 'faster-whisper-xxl':
                return True
        return False
    
    def setup_faster_whisper_xxl_button(self):
        self.has_faster_whisper_xxl = self.check_faster_whisper_xxl_folder()
        
        if not self.has_faster_whisper_xxl:
            self.faster_whisper_xxl_btn.setStyleSheet("background-color: #FF0000; color: white;")
        # No configuramos aquí el estilo para cuando está disponible, lo haremos en set_model
        
        self.faster_whisper_xxl_btn.clicked.connect(self.on_faster_whisper_xxl_clicked)
        
    def on_faster_whisper_xxl_clicked(self):
        if not self.has_faster_whisper_xxl:
            self.output_text.append("Para usar Faster-Whisper XXL, descargue el último release de "
                                    "github.com/Purfview/whisper-standalone-win y extraiga todo "
                                    "con la carpeta llamada 'Faster-Whisper-XXL' en el directorio del proyecto.")
        else:
            self.selected_model = "faster-whisper-xxl"
            for button in self.temp_buttons.buttons():
                button.setEnabled(False)

    def on_directory_changed(self, path):
        self.populate_tree_view()

    def setup_sidebar(self):
        # Crear el widget principal
        main_widget = QWidget()
        main_layout = QHBoxLayout(main_widget)
        main_layout.setContentsMargins(0, 0, 0, 0)
        main_layout.setSpacing(0)

        # Crear el contenedor central
        self.central_container = QWidget()
        self.central_layout = QVBoxLayout(self.central_container)
        self.central_layout.addWidget(self.central_widget)

        # Crear el sidebar
        self.sidebar = QWidget()
        self.sidebar.setFixedWidth(250)  # Ajusta este valor según lo necesites
        self.sidebar_layout = QVBoxLayout(self.sidebar)
        
        # Configurar la vista de árbol
        self.dir_model = QStandardItemModel()
        self.tree_view = QTreeView()
        self.tree_view.setIndentation(10)  # Reducir la indentación
        self.tree_view.setStyleSheet("""
            QTreeView::branch {
                background: transparent;
            }
            QTreeView::item {
                padding-left: 0px;
            }
        """)

        # Configurar los íconos personalizados
        self.dir_model.setHorizontalHeaderLabels(["Nombre"])
        self.tree_view.setIconSize(QSize(16, 16))  # Ajustar el tamaño de los íconos si es necesario

        # Modificar la barra superior del sidebar
        sidebar_top_layout = QHBoxLayout()

        # Título "Transcripciones"
        title_label = QLabel("Transcripciones")
        title_font = QFont()
        title_font.setPointSize(12)  # Aumentar el tamaño de la fuente
        title_font.setBold(True)
        title_label.setFont(title_font)
        sidebar_top_layout.addWidget(title_label)

        self.sidebar_layout.addLayout(sidebar_top_layout)
        
        sidebar_top_layout.addStretch()  # Esto empujará el botón de actualizar hacia la derecha
        
        self.sidebar_layout.addLayout(sidebar_top_layout)
        self.tree_view.setModel(self.dir_model)
        self.tree_view.setHeaderHidden(True)
        self.tree_view.setAnimated(False)
        self.tree_view.setIndentation(20)
        self.tree_view.setSortingEnabled(True)
        self.tree_view.setEditTriggers(QTreeView.EditTrigger.NoEditTriggers)
        self.tree_view.doubleClicked.connect(self.open_selected_file)

        self.tree_view.expanded.connect(self.on_item_expanded)
        self.tree_view.collapsed.connect(self.on_item_collapsed)

        # Poblar el modelo con archivos
        self.populate_tree_view()

        self.sidebar_layout.addWidget(self.tree_view)

        # Botones de 'Abrir' y 'Limpiar transcripciones'
        button_layout = QHBoxLayout()
        self.open_btn = QPushButton("Abrir")
        self.open_btn.clicked.connect(self.open_selected_file)
        button_layout.addWidget(self.open_btn)
        
        self.clean_btn = QPushButton("Limpiar transcripciones")
        self.clean_btn.clicked.connect(self.clean_transcriptions)
        button_layout.addWidget(self.clean_btn)
        
        self.sidebar_layout.addLayout(button_layout)

        # Crear el botón de toggle
        self.toggle_sidebar_btn = QPushButton(">")
        self.toggle_sidebar_btn.setFixedSize(20, 60)
        self.toggle_sidebar_btn.setStyleSheet("background-color: #4A4A4A;")
        self.toggle_sidebar_btn.clicked.connect(self.toggle_sidebar)

        # Agregar widgets al layout principal
        main_layout.addWidget(self.central_container)
        main_layout.addWidget(self.toggle_sidebar_btn)
        main_layout.addWidget(self.sidebar)

        # Ocultar inicialmente el sidebar
        self.sidebar.hide()

        self.setCentralWidget(main_widget)

    def toggle_sidebar(self):
        current_width = self.width()
        if self.sidebar.isVisible():
            self.sidebar.hide()
            self.toggle_sidebar_btn.setText(">")
            self.resize(current_width - self.sidebar.width(), self.height())
        else:
            self.sidebar.show()
            self.toggle_sidebar_btn.setText("<")
            self.resize(current_width + self.sidebar.width(), self.height())

    def populate_tree_view(self):
        root_dir = os.path.join(self.base_dir, "transcription_results")
        self.dir_model.clear()
        self.add_directory(root_dir, self.dir_model.invisibleRootItem())
        
        # Expandir todas las carpetas
        self.tree_view.expandAll()
        
        # Actualizar el watcher con todos los subdirectorios
        for dirpath, dirnames, filenames in os.walk(root_dir):
            try:
                if os.path.exists(dirpath):
                    self.file_watcher.addPath(dirpath)
            except Exception as e:
                print(f"Error al agregar directorio al watcher: {e}")
        # Forzar actualización de la vista
        self.tree_view.viewport().update()

    def add_directory(self, path, parent):
        directories = []
        files = []
        
        for item in os.listdir(path):
            item_path = os.path.join(path, item)
            if os.path.isdir(item_path):
                dir_item = QStandardItem(QIcon("./images/folder.png"), item)
                directories.append((dir_item, item_path))
            elif os.path.isfile(item_path) and item.endswith('.txt'):
                file_item = QStandardItem(QIcon("./images/file.png"), item)
                files.append(file_item)
        
        # Agregar primero las carpetas
        for dir_item, dir_path in directories:
            parent.appendRow(dir_item)
            self.add_directory(dir_path, dir_item)
        
        # Luego agregar los archivos
        for file_item in files:
            parent.appendRow(file_item)
        
        # Expandir la carpeta actual
        if isinstance(parent, QStandardItem):
            index = self.dir_model.indexFromItem(parent)
            self.tree_view.expand(index)
            
        # Forzar la actualización de los iconos
        self.tree_view.viewport().update()

    def on_item_expanded(self, index):
        item = self.dir_model.itemFromIndex(index)
        item.setIcon(QIcon("./images/open-folder.png"))

    def on_item_collapsed(self, index):
        item = self.dir_model.itemFromIndex(index)
        item.setIcon(QIcon("./images/folder.png"))

    def open_selected_file(self):
        index = self.tree_view.currentIndex()
        if index.isValid():
            item = self.dir_model.itemFromIndex(index)
            file_path = os.path.join(self.base_dir, "transcription_results", item.text())
            if os.path.isfile(file_path) and file_path.endswith('.txt'):
                self.open_timestamp_notepad(file_path)
            else:
                print(f"Archivo no válido o no existe: {file_path}")

    def open_timestamp_notepad(self, file_path):
        subprocess.Popen([sys.executable, "timestamp_notepad.py", file_path])

    def clean_transcriptions(self):
        source_dir = os.path.join(self.base_dir, "transcription_results")
        target_dir = os.path.join(self.base_dir, "results_trashcan")
        
        if not os.path.exists(target_dir):
            os.makedirs(target_dir)
        
        def move_txt_files(src, dst):
            for root, dirs, files in os.walk(src):
                for file in files:
                    if file.endswith('.txt'):
                        rel_path = os.path.relpath(root, src)
                        dst_path = os.path.join(dst, rel_path)
                        if not os.path.exists(dst_path):
                            os.makedirs(dst_path)
                        shutil.move(os.path.join(root, file), os.path.join(dst_path, file))
        
        move_txt_files(source_dir, target_dir)
    
        self.populate_tree_view()  # Actualizar la vista de árbol
        QMessageBox.information(self, "Limpieza completada", "Las transcripciones han sido movidas a 'results_trashcan'.")

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

    def update_progress_bar(self, start, end, audio_file):
        start_seconds = self.time_to_seconds(start)
        end_seconds = self.time_to_seconds(end)
        total_duration = self.get_audio_duration(audio_file)
        
        if total_duration > 0:
            progress = min((end_seconds / total_duration) * 100, 100)  # Asegurarse de que no exceda el 100%
            self.progress_bar.setValue(int(progress))
            self.progress_bar.setFormat(f"{progress:.2f}%")

    def time_to_seconds(self, time_str):
        minutes, seconds = time_str.split(':')
        return int(minutes) * 60 + float(seconds)

    def get_audio_duration(self, audio_file):
        for file, duration in self.audio_files:
            if audio_file in file:
                return duration
        return 0
    
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
        
        # Mostrar mensaje de carga
        self.loading_label = QLabel("Buscando archivos de audio...", self)
        self.loading_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.loading_label.setStyleSheet("background-color: rgba(0, 0, 0, 150); color: white; padding: 10px;")
        self.loading_label.resize(300, 50)
        self.loading_label.move((self.width() - self.loading_label.width()) // 2,
                                (self.height() - self.loading_label.height()) // 2)
        self.loading_label.show()
        
        # Deshabilitar controles
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
        self.loading_label.hide()
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
        
        self.update_button_states()

    def set_translation(self, translate):
        self.translate = translate
        self.translate_btn.setChecked(translate)
        self.no_translate_btn.setChecked(not translate)

    def update_button_states(self):
        is_spanish = self.current_language == 'es'
        self.translate_btn.setEnabled(is_spanish)
        self.no_translate_btn.setEnabled(is_spanish)
        
        if is_spanish:
            if not self.translate_btn.isChecked() and not self.no_translate_btn.isChecked():
                self.translate_btn.setChecked(True)
                self.translate = True
        else:
            self.translate_btn.setChecked(False)
            self.no_translate_btn.setChecked(False)
            self.translate = False
        
        # Asegurarse de que siempre haya una opción seleccionada cuando el idioma es español
        if is_spanish and not self.translate_btn.isChecked() and not self.no_translate_btn.isChecked():
            self.translate_btn.setChecked(True)
            self.translate = True
    
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

        if self.selected_model == "faster-whisper-xxl":
            if not self.has_faster_whisper_xxl:
                self.output_text.append("Faster-Whisper XXL no está disponible. Por favor, descárguelo primero.")
                return
            self.transcribe_with_faster_whisper_xxl(selected)
        else:
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
    
    def transcribe_with_faster_whisper_xxl(self, selected=True):
        files_to_transcribe = [item.text().split(' | ', 1)[1] for item in self.file_list.selectedItems()] if selected else [self.file_list.item(i).text().split(' | ', 1)[1] for i in range(self.file_list.count())]
        
        if not files_to_transcribe:
            self.output_text.append("No se han seleccionado archivos para transcribir.")
            return

        self.output_text.append("Iniciando transcripción con Faster-Whisper XXL...")
        base_output_dir = os.path.join(self.base_dir, "transcription_results")

        executable_path = os.path.abspath(os.path.join(self.base_dir, "Faster-Whisper-XXL", "faster-whisper-xxl.exe"))
        
        if not os.path.exists(executable_path):
            self.output_text.append(f"Error: No se encontró el ejecutable en {executable_path}")
            return

        # Iniciar el temporizador
        self.elapsed_seconds = 0
        self.elapsed_timer.start(1000)
        
        # Configurar la barra de progreso
        self.progress_bar.setValue(0)
        self.progress_bar.setVisible(True)
        self.progress_bar.setFormat("0.00%")

        self.transcription_thread = SequentialTranscriptionThread(
            files_to_transcribe, executable_path, base_output_dir, 
            self.folder_input.text(), self.current_language, self.translate
        )
        self.transcription_thread.transcription_started.connect(lambda file: self.output_text.append(f"Iniciando transcripción para: {file}"))
        self.transcription_thread.output_received.connect(self.update_output)
        self.transcription_thread.progress_update.connect(self.update_progress_bar)
        self.transcription_thread.transcription_finished.connect(self.on_transcription_finished)
        self.transcription_thread.finished.connect(self.on_all_transcriptions_finished)
        
        self.transcription_thread.start()

        self.transcribe_all_btn.setEnabled(False)
        self.transcribe_selected_btn.setEnabled(False)

    def on_all_transcriptions_finished(self):
        self.output_text.append("Todas las transcripciones han sido completadas.")
        self.transcribe_all_btn.setEnabled(True)
        self.transcribe_selected_btn.setEnabled(True)
        self.elapsed_timer.stop()
        self.progress_bar.setValue(100)
        self.progress_bar.setFormat("100%")

    def update_output(self, line):
        if self.selected_model == "faster-whisper-xxl":
            print(line)  # Imprimir en la consola normal
        else:
            self.output_text.append(line)

    def on_transcription_finished(self, audio_file, success):
        if success:
            self.output_text.append(f"Transcripción completada para: {audio_file}")
            self.output_text.append(f"-------------------------------------------")
            self.populate_tree_view()
        else:
            self.output_text.append(f"Error al transcribir {audio_file}")

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