import QtQuick
import QtQuick.Window
import QtQuick.Layouts
import QtQuick.Controls

Item {
    height: 60
    id: header
    clip: true

    Layout.fillWidth: true

    Row {
        // anchors.centerIn: parent
        // anchors.fill: parent
        anchors.right: parent.right
        anchors.leftMargin: 20
        anchors.topMargin: 20
        anchors.rightMargin: 20
        spacing: 10
        height: 80
        visible: loginModel.loggedIn

        Label {
            text: "Logged in as " + loginModel.username
            anchors.verticalCenter: parent.verticalCenter
        }

        Button {
            text: "Logout"
            onClicked: loginModel.doLogout()
            anchors.verticalCenter: parent.verticalCenter
        }
    }
}