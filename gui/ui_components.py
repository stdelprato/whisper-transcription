from PyQt6.QtCore import Qt
from PyQt6.QtWidgets import QPushButton, QButtonGroup, QFrame, QLabel, QLineEdit, QListWidget, QTextEdit, QSlider, QProgressBar, QHBoxLayout

def setup_ui(window):
    folder_layout = QHBoxLayout()
    window.folder_input = QLineEdit()
    window.folder_input.setReadOnly(True)
    folder_layout.addWidget(QLabel("Carpeta de audio:"))
    folder_layout.addWidget(window.folder_input)
    window.browse_btn = QPushButton("Explorar")
    folder_layout.addWidget(window.browse_btn)
    window.layout.addLayout(folder_layout)

    window.file_list = QListWidget()
    window.file_list.setSelectionMode(QListWidget.SelectionMode.MultiSelection)
    window.layout.addWidget(window.file_list)

    # Botones de idioma
    lang_layout = QHBoxLayout()
    window.lang_group = QButtonGroup(window)
    window.es_btn = QPushButton("Español")
    window.en_btn = QPushButton("Inglés")
    window.auto_btn = QPushButton("Auto-detectar")
    window.auto_detect_btn = QPushButton("Auto-detectar y traducir")
    
    buttons = [window.es_btn, window.en_btn, window.auto_btn, window.auto_detect_btn]
    for btn in buttons:
        btn.setCheckable(True)
        window.lang_group.addButton(btn)
        lang_layout.addWidget(btn)
    
    window.layout.addLayout(lang_layout)

    # Botones de traducción
    trans_layout = QHBoxLayout()
    window.translate_btn = QPushButton("Traducir a Inglés")
    window.no_translate_btn = QPushButton("No traducir a Inglés")
    window.translate_btn.setCheckable(True)
    window.no_translate_btn.setCheckable(True)
    trans_layout.addWidget(window.translate_btn)
    trans_layout.addWidget(window.no_translate_btn)
    window.layout.addLayout(trans_layout)

    # Agregar separador
    separator = QFrame()
    separator.setFrameShape(QFrame.Shape.HLine)
    separator.setFrameShadow(QFrame.Shadow.Sunken)
    window.layout.addWidget(separator)

    # Botones de selección de modelo
    model_layout = QHBoxLayout()
    window.model_group = QButtonGroup(window)
    window.faster_whisper_btn = QPushButton("Faster-Whisper")
    window.original_whisper_btn = QPushButton("Whisper Original")
    window.faster_whisper_btn.setCheckable(True)
    window.original_whisper_btn.setCheckable(True)
    window.faster_whisper_btn.setChecked(True)  # Faster-Whisper por defecto
    window.model_group.addButton(window.faster_whisper_btn)
    window.model_group.addButton(window.original_whisper_btn)
    model_layout.addWidget(window.faster_whisper_btn)
    model_layout.addWidget(window.original_whisper_btn)
    window.layout.addLayout(model_layout)

    # Botones de transcripción
    window.trans_btn_layout = QHBoxLayout()
    window.transcribe_selected_btn = QPushButton("Transcribir seleccionados")
    window.transcribe_all_btn = QPushButton("Transcribir todos")
    window.transcribe_selected_btn.setVisible(False)
    window.trans_btn_layout.addWidget(window.transcribe_selected_btn)
    window.trans_btn_layout.addWidget(window.transcribe_all_btn)
    window.layout.addLayout(window.trans_btn_layout)

    window.progress_bar = QProgressBar()
    window.progress_bar.setVisible(False)
    window.layout.addWidget(window.progress_bar)

    window.output_text = QTextEdit()
    window.output_text.setReadOnly(True)
    window.layout.addWidget(window.output_text)

    window.clear_btn = QPushButton("Limpiar resultados")
    window.layout.addWidget(window.clear_btn)

def setup_connections(window):
    window.browse_btn.clicked.connect(window.browse_folder)
    
    # Conectar el grupo de botones de idioma
    window.lang_group.buttonClicked.connect(window.on_lang_button_clicked)
    
    # Mantener las conexiones individuales para compatibilidad
    window.es_btn.clicked.connect(lambda: window.on_lang_button_clicked(window.es_btn))
    window.en_btn.clicked.connect(lambda: window.on_lang_button_clicked(window.en_btn))
    window.auto_btn.clicked.connect(lambda: window.on_lang_button_clicked(window.auto_btn))
    window.auto_detect_btn.clicked.connect(lambda: window.on_lang_button_clicked(window.auto_detect_btn))
    
    # Conexiones para los botones de traducción
    window.translate_btn.clicked.connect(lambda: window.set_translation(True))
    window.no_translate_btn.clicked.connect(lambda: window.set_translation(False))
    
    # Conexiones para los botones de transcripción
    window.transcribe_selected_btn.clicked.connect(lambda: window.transcribe(selected=True))
    window.transcribe_all_btn.clicked.connect(lambda: window.transcribe(selected=False))
    
    # Conexiones para la lista de archivos
    window.file_list.itemSelectionChanged.connect(window.update_transcribe_buttons)
    window.file_list.itemSelectionChanged.connect(window.update_estimate)
    
    # Conexión para el botón de limpiar
    window.clear_btn.clicked.connect(window.clear_output)

def set_style(window):
    base_style = """
    QPushButton#transcribe_selected_btn { 
        background-color: #1a5f7a; 
        color: white;
    }
    QPushButton#transcribe_all_btn { 
        background-color: #2d8659; 
        color: white;
    }
    """
    window.setStyleSheet("""
        QWidget { background-color: #2D2D2D; color: #FFFFFF; }
        QPushButton { background-color: #4A4A4A; border: 1px solid #5A5A5A; padding: 5px; }
        QPushButton:hover { background-color: #5A5A5A; }
        QPushButton:checked { background-color: #3A7CA5; }
        QLineEdit, QTextEdit, QListWidget { 
            background-color: #3D3D3D; 
            border: 1px solid #5A5A5A; 
            padding: 5px; 
        }
        QListWidget::item:selected { 
            background-color: #4A90E2; 
            color: white;
        }
        QListWidget::item:hover {
            background-color: #5A5A5A;
        }
        QListWidget::item:selected:hover {
            background-color: #3A7CA5;
        }
        QPushButton:disabled { 
            background-color: #2D2D2D; 
            color: #666666; 
            border: 1px solid #3D3D3D;
        }
        QSlider::groove:horizontal {
            border: 1px solid #999999;
            height: 8px;
            background: #4A4A4A;
            margin: 2px 0;
        }
        QSlider::handle:horizontal {
            background: #4A90E2;
            border: 1px solid #5A5A5A;
            width: 18px;
            margin: -2px 0;
            border-radius: 3px;
        }
    """)
    window.setStyleSheet(window.styleSheet() + base_style)
        
    window.transcribe_selected_btn.setObjectName("transcribe_selected_btn")
    window.transcribe_all_btn.setObjectName("transcribe_all_btn")