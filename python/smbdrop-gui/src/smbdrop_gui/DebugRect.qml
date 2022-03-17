import QtQuick 2.12
// stolen from https://stackoverflow.com/a/60641874/540644

Rectangle {
  property var toFill: parent          // instantiation site "can" (optionally) override
  property color customColor: 'yellow' // instantiation site "can" (optionally) override
  property int customThickness: 1      // instantiation site "can" (optionally) override

  anchors.fill: toFill
  z: 200
  color: 'transparent'
  border.color: customColor
  border.width: customThickness
}