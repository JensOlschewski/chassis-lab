% storeHistory updates a structure with variable data at a given index
%
% Dynamically stores variable numbers of data vectors into the corresponding fields 
% of a history structure at a specified index. Fields are preallocated for improved performance.
%
% Inputs:
%   historyStruct - Structure to be updated with new data
%   steps - Total number of steps for preallocation
%   index - Current step index for data storage
%   varargin - Pairs of [field name, data vector]
%
% Output:
%   historyStruct - Updated history structure with new data

function historyStruct = storeHistory(historyStruct, index,steps, varargin)
    % Preallocate fields if first index
    if index == 1
        for i = 1:2:length(varargin)
            varName = varargin{i};  % Field name
            varData = varargin{i + 1};  % Corresponding data
            if ~isempty(varName) && ~isempty(varData)
                % Preallocate space based on the size of varData and total steps
                historyStruct.(varName) = zeros(size(varData, 1), steps);
            end
        end
    end

    % Store each variable in its respective field at the given index
    for i = 1:2:length(varargin)
        varName = varargin{i};  % Field name
        varData = varargin{i + 1};  % Corresponding data
        if ~isempty(varName) && ~isempty(varData)
            historyStruct.(varName)(:, index) = varData;
        end
    end
end
