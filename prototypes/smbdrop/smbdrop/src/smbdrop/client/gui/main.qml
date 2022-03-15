import QtQuick
import QtQuick.Layouts
import QtQuick.Window
import QtQuick.Controls

ApplicationWindow {
    id: root

    title: "ER-C-Data Ingest"
    visible: true
    width: 640
    height: 480

    ColumnLayout {
        anchors.fill: parent

        Header { }
        Loader {
            id: pageLoader
            source: pageStateModel.currentPage
            Layout.fillHeight: true
            Layout.fillWidth: true
        }
    }

    // make the page state model aware of the current login state:
    Connections {
        target: loginModel
        function onLoggedInChanged() {
            pageStateModel.loginStateChanged()
        }
    }
}
