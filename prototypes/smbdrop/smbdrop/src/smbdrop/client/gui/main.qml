import QtQuick
import QtQuick.Window
import QtQuick.Controls

ApplicationWindow {
    id: root

    title: "ER-C Cloud Ingest"
    visible: true
    width: 640
    height: 480

    header: Header { }

    Loader {
        id: pageLoader
        source: pageStateModel.currentPage
        anchors.fill: parent
    }

    // make the page state model aware of the current login state:
    Connections {
        target: loginModel
        function onLoggedInChanged() {
            pageStateModel.loginStateChanged()
        }
    }
}
