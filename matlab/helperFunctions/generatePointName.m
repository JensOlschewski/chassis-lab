% Helper function for point name generation
function pointName = generatePointName(baseName, bodyID, part_side, isGround)
    if isGround
        bodyID = 0;
        suffix = ['_' part_side];
    else
        suffix = '';
    end
    pointName = [lower(baseName) num2str(bodyID) suffix];
end