import QtQuick
import QtQuick.Window
import QtQuick.Controls

Item {
    visible: loginModel.loggedIn

    Row {
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.topMargin: 10
        anchors.rightMargin: 10
        spacing: 10

        Text {
            text: "Logged in as " + loginModel.username
            anchors.verticalCenter: parent.verticalCenter
        }

        Button {
            text: "Logout"
            onClicked: loginModel.doLogout()
        }
    }
}