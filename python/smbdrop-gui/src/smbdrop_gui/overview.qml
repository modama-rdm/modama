import QtQuick
import QtQuick.Window
import QtQuick.Layouts
import QtQuick.Controls

Item {
    id: page
    visible: true
    width: parent.width

    Dialog {
        id: closeSessionDialog
        onAccepted: sessionModel.closeSession(sessionId)
        standardButtons: Dialog.Ok | Dialog.Cancel
        modal: true
        anchors.centerIn: parent
        width: 300

        title: "Close session?"

        Label {
            text: "Really close session " + closeSessionDialog.sessionTitle + "?"
        }

        property string sessionId: ""
        property string sessionTitle: ""
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.topMargin: 20
        anchors.leftMargin: 20
        anchors.rightMargin: 20
        spacing: 20

        Pane {
            Layout.fillWidth: true
            Material.elevation: 5

            ColumnLayout {
                Label {
                    text: qsTr("Current Session:")
                }

                ButtonRow {
                    Button {
                        id: openSessionButton
                        text: qsTr("New Session")
                        onClicked: pageStateModel.startSessionDialog()
                    }
                }
            }
        }

        Label {
            text: "Your other open sessions:"
            Layout.fillWidth: true
            visible: sharesModel.rowCount > 0
        }

        Label {
            text: "You don't have any other sessions open"
            Layout.fillWidth: true
            visible: sharesModel.rowCount == 0
        }

        ListView {
            spacing: 20
            flickableDirection: Flickable.VerticalFlick
            boundsBehavior: Flickable.StopAtBounds
            clip: true
            model: sharesModel

            Layout.fillHeight: true
            Layout.fillWidth: true

            header: Item {
                height: 20
            }

            // FIXME: show mapped drive name
            delegate: Pane {
                // anchors.horizontalCenter: ListView.view.horizontalCenter
                width: ListView.view.width - 10

                Material.elevation: 5

                ColumnLayout {
                    Label {
                        text: model.title
                        font.pixelSize: 22
                        bottomPadding: 10
                    }
                    Label {
                        text: "Created: " + model.created_datetime
                        font.pixelSize: 14
                        bottomPadding: 10
                    }
                    ButtonRow {
                        Button {
                            id: sealButton
                            text: qsTr("Close")
                            onClicked: function() {
                                closeSessionDialog.sessionId = model.share_id;
                                closeSessionDialog.sessionTitle = model.title;
                                closeSessionDialog.open();
                            }
                        }

                        Button {
                            id: mapSessionDriveButton
                            text: qsTr("Map Drive")
                            onClicked: sessionModel.mapSessionDrive(model.share_id)
                            enabled: !sessionModel.busy
                        }
                    }
                }
            }

            ScrollBar.vertical: ScrollBar { }
        }
    }
}
