import os
os.environ['HF_HUB_DISABLE_SYMLINKS_WARNING'] = '1'

import sys
from PyQt6.QtWidgets import QApplication

if __name__ == '__main__':
    app = QApplication(sys.argv)
    from gui.main_window import MainWindow
    window = MainWindow()
    window.show()
    sys.exit(app.exec())